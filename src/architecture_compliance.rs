use std::fmt;

const BANNED_DEPENDENCIES: &[&str] = &[
    "wgpu", "egui", "eframe", "iced", "tao", "winit", "gtk", "gpui", "sdl2", "glium",
    "glutin", "skia", "ash", "vulkano",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchitectureComplianceError {
    violations: Vec<String>,
}

impl ArchitectureComplianceError {
    pub fn violations(&self) -> &[String] {
        &self.violations
    }
}

impl fmt::Display for ArchitectureComplianceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "TUI-only architecture compliance failed for banned dependencies: {}",
            self.violations.join(", ")
        )
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ArchitectureComplianceGuard;

impl ArchitectureComplianceGuard {
    pub fn verify_dependencies(
        &self,
        manifest: &str,
        lockfile: &str,
    ) -> Result<(), ArchitectureComplianceError> {
        let manifest_tokens = dependency_tokens(manifest);
        let lockfile_tokens = dependency_tokens(lockfile);
        let mut violations = Vec::new();

        for dependency in BANNED_DEPENDENCIES {
            if manifest_tokens.iter().any(|token| token == dependency) {
                violations.push(format!("{dependency} (Cargo.toml)"));
            }
            if lockfile_tokens.iter().any(|token| token == dependency) {
                violations.push(format!("{dependency} (Cargo.lock)"));
            }
        }

        violations.sort();
        violations.dedup();

        if violations.is_empty() {
            Ok(())
        } else {
            Err(ArchitectureComplianceError { violations })
        }
    }
}

fn dependency_tokens(contents: &str) -> Vec<String> {
    contents
        .lines()
        .flat_map(|line| {
            let line = line.split('#').next().unwrap_or("");
            line.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'))
                .filter(|token| !token.is_empty())
                .map(|token| token.to_ascii_lowercase())
                .collect::<Vec<_>>()
        })
        .collect()
}
