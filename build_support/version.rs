//! Base-100 dotted program versions used by the build script.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Version {
    pub major: u64,
    pub minor: u8,
    pub patch: u8,
}

impl Version {
    pub fn parse(value: &str) -> Result<Self, String> {
        let parts: Vec<_> = value.trim().split('.').collect();
        if parts.len() != 3 {
            return Err("version must contain exactly three numeric components".into());
        }
        let major = parts[0]
            .parse::<u64>()
            .map_err(|_| "major version must be a non-negative integer")?;
        let minor = parse_subversion(parts[1], "minor")?;
        let patch = parse_subversion(parts[2], "patch")?;
        Ok(Self {
            major,
            minor,
            patch,
        })
    }

    pub fn increment(self) -> Self {
        if self.patch < 99 {
            Self {
                patch: self.patch + 1,
                ..self
            }
        } else if self.minor < 99 {
            Self {
                minor: self.minor + 1,
                patch: 0,
                ..self
            }
        } else {
            Self {
                major: self.major + 1,
                minor: 0,
                patch: 0,
            }
        }
    }
}

fn parse_subversion(value: &str, name: &str) -> Result<u8, String> {
    let parsed = value
        .parse::<u8>()
        .map_err(|_| format!("{name} version must be an integer in [0, 99]"))?;
    if parsed > 99 {
        return Err(format!("{name} version must be an integer in [0, 99]"));
    }
    Ok(parsed)
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn increments_patch_then_minor_then_major_at_one_hundred() {
        assert_eq!(
            Version::parse("0.0.0").unwrap().increment().to_string(),
            "0.0.1"
        );
        assert_eq!(
            Version::parse("0.0.99").unwrap().increment().to_string(),
            "0.1.0"
        );
        assert_eq!(
            Version::parse("0.99.99").unwrap().increment().to_string(),
            "1.0.0"
        );
    }

    #[test]
    fn rejects_components_outside_template_range() {
        assert!(Version::parse("0.100.0").is_err());
        assert!(Version::parse("0.0.100").is_err());
        assert!(Version::parse("0.1").is_err());
    }
}
