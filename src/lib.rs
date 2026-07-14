use serde::Serialize;
use std::io::{BufRead, BufReader, BufWriter, Write};

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
#[must_use]
/// This structure represents the metadata that will be baked into your executable.
pub struct PackageMetadata {
    pub name: String,
    pub binary: String,
    pub version: String,
    pub cpe: String,
    pub app_cpe: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    pub license: String,
    #[serde(rename = "type")]
    pub record_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maintainer: Option<String>,
    pub copyright: String,
    pub os: String,
    pub architecture: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_version: Option<String>,
}

const OWNER: [u8; 4] = [0x46, 0x44, 0x4f, 0x00];

impl PackageMetadata {
    #[allow(unused)]
    fn write_linker_script<W: Write>(self, target: W) -> std::io::Result<()> {
        let mut bw = BufWriter::new(target);
        let mut serialized = serde_json::to_vec(&self)?;
        while serialized.len() % 4 != 0 {
            serialized.push(0x00);
        }
        writeln!(&mut bw, "SECTIONS")?;
        writeln!(&mut bw, "{{")?;
        // TODO: we should set (READONLY) here, but it only works on GNU LD, not on LLD
        writeln!(&mut bw, "    .note.package : ALIGN(4)")?;
        writeln!(&mut bw, "    {{")?;
        writeln!(&mut bw, "        KEEP(*(.note.package))")?;
        writeln!(&mut bw, "        LONG({:#04x})", OWNER.len())?;
        writeln!(&mut bw, "        LONG({:#04x})", serialized.len())?;
        writeln!(&mut bw, "        LONG(0xcafe1a7e)")?; // magic number from spec
        write_bytes(&mut bw, &OWNER)?;
        write_bytes(&mut bw, &serialized)?;
        writeln!(&mut bw, "    }}")?;
        writeln!(&mut bw, "}}")?;
        writeln!(&mut bw, "INSERT AFTER .note.gnu.build-id;")?;
        bw.flush()?;
        Ok(())
    }

    #[cfg(target_os = "linux")]
    pub fn write_linker_script_and_inject_argument(self) -> std::io::Result<()> {
        let target = std::path::PathBuf::from(
            std::env::var_os("OUT_DIR").expect("Cargo always sets OUT_DIR"),
        )
        .join("package_notes.ld");
        let writer = std::fs::File::create(&target)?;
        self.write_linker_script(writer)?;
        println!("cargo:rustc-link-arg=-Wl,-dT,{}", target.display());
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    pub fn write_linker_script_and_inject_argument(self) -> std::io::Result<()> {
        Ok(())
    }

    /// Create a new builder for this type
    pub fn builder(vendor: impl Into<String>) -> Result<PackageMetadataBuilder, &'static str> {
        PackageMetadataBuilder::new_from_cargo(vendor)
    }
}

#[derive(Debug)]
pub struct PackageMetadataBuilder {
    name: String,
    binary: Option<String>,
    vendor: String,
    version: String,
    cpe: Option<String>,
    hash: Option<String>,
    license: Option<String>,
    record_type: String,
    maintainer: Option<String>,
    copyright: Option<String>,
    os: String,
    os_version: Option<String>,
    architecture: &'static str,
}

impl PackageMetadataBuilder {
    /// Construct a new PackageMetadataBuilder reading the environment variables injected by Cargo
    pub fn new_from_cargo(vendor: impl Into<String>) -> Result<Self, &'static str> {
        let name = std::env::var("CARGO_PKG_NAME").map_err(|_| "CARGO_PKG_NAME unset")?;
        let version = std::env::var("CARGO_PKG_VERSION").map_err(|_| "CARGO_PKG_VERSION unset")?;
        let mut builder = Self::new(name.clone(), vendor, version)
            .binary(std::env::var("CARGO_BIN_NAME").unwrap_or_else(|_| name.clone()));
        if let Ok(val) = std::env::var("CARGO_PKG_LICENSE") {
            builder = builder.license(val);
        }
        Ok(builder)
    }

    /// Construct a new empty PackageMetadataBuilder
    pub fn new(
        name: impl Into<String>,
        vendor: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        let name = name.into();
        let vendor = vendor.into();
        let version = version.into();
        let (os, os_version) = get_os_metadata().unwrap_or_else(|_| ("Unknown".to_string(), None));
        Self {
            name,
            version,
            binary: None,
            vendor,
            cpe: None,
            hash: None,
            license: None,
            record_type: "Unknown".to_string(),
            maintainer: None,
            copyright: None,
            os,
            os_version,
            architecture: std::env::consts::ARCH,
        }
    }

    /// Set the application name
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Set this specific binary's name
    pub fn binary(mut self, binary: impl Into<String>) -> Self {
        self.binary = Some(binary.into());
        self
    }

    /// Set the full application version
    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self
    }

    /// Set the cpe and appCpe fields. If unset, this will be computed from the other fields
    pub fn cpe(mut self, cpe: impl Into<String>) -> Self {
        self.cpe = Some(cpe.into());
        self
    }

    /// Set the VCS hash
    pub fn hash(mut self, hash: impl Into<String>) -> Self {
        self.hash = Some(hash.into());
        self
    }

    /// Set the license, ideally as an SPDX string
    pub fn license(mut self, license: impl Into<String>) -> Self {
        self.license = Some(license.into());
        self
    }

    /// Set the copyright string
    pub fn copyright(mut self, copyright: impl Into<String>) -> Self {
        self.copyright = Some(copyright.into());
        self
    }

    /// Set the opaque maintainer string
    pub fn maintainer(mut self, maintainer: impl Into<String>) -> Self {
        self.maintainer = Some(maintainer.into());
        self
    }

    /// Build this into a PackageMetadata, returning any errors
    ///
    /// You should call .writer_linker_script_and_inject_argument() on the resultant value.
    pub fn build(self) -> Result<PackageMetadata, &'static str> {
        let short_release_version = self
            .version
            .split_once("-")
            .map(|s| s.0)
            .unwrap_or(&self.version);
        let cpe = self.cpe.unwrap_or_else(|| {
            format!(
                "cpe:2.3:a:{}:{}:{short_release_version}:*:*:*:*:*:*:*",
                self.vendor, self.name
            )
        });
        Ok(PackageMetadata {
            binary: self.binary.unwrap_or_else(|| self.name.clone()),
            name: self.name,
            version: self.version,
            cpe: cpe.clone(),
            app_cpe: cpe,
            hash: self.hash,
            license: self.license.ok_or("missing license")?,
            record_type: self.record_type,
            maintainer: self.maintainer,
            copyright: self.copyright.ok_or("missing copyright")?,
            os: self.os,
            os_version: self.os_version,
            architecture: self.architecture,
        })
    }
}

/// Read linux metadata from /etc/os-release
fn get_os_metadata() -> Result<(String, Option<String>), Box<dyn std::error::Error>> {
    let f = std::fs::File::open("/etc/os-release")?;
    let f = BufReader::new(f);
    let mut os = None;
    let mut os_family = None;
    for line in f.lines() {
        let line = line?;
        if line.trim_start().starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once("=") else {
            continue;
        };
        let value = value
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .unwrap_or(value);
        if key == "ID" {
            os = Some(value.to_string());
        } else if key == "VERSION_ID" {
            os_family = Some(value.to_string())
        }
    }
    if let Some(os) = os {
        Ok((os, os_family))
    } else {
        Ok((std::env::consts::OS.to_string(), None))
    }
}

fn write_bytes<W: std::io::Write>(writer: &mut W, bytes: &[u8]) -> std::io::Result<()> {
    for byte in bytes {
        writeln!(writer, "        BYTE({byte:#02x})")?;
    }
    Ok(())
}
