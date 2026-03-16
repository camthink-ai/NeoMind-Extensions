use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored::Colorize;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[derive(Parser)]
#[command(name = "neomind-ext")]
#[command(about = "CLI tool for developing NeoMind extensions", long_about = None)]
#[command(version = "0.1.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new extension project
    New {
        /// Extension name (kebab-case, e.g., my-cool-extension)
        name: String,
        /// Include frontend component
        #[arg(long)]
        with_frontend: bool,
    },
    /// Build extension
    Build {
        /// Extension directory (default: current directory)
        #[arg(short, long)]
        path: Option<String>,
        /// Release build
        #[arg(long)]
        release: bool,
    },
    /// Package extension as .nep file
    Package {
        /// Extension directory
        #[arg(short, long)]
        path: Option<String>,
        /// Include frontend
        #[arg(long)]
        with_frontend: bool,
    },
    /// Validate extension or .nep package
    Validate {
        /// Extension path or .nep file
        #[arg(short, long)]
        path: Option<String>,
    },
    /// Test extension
    Test {
        /// Extension directory
        #[arg(short, long)]
        path: Option<String>,
        /// Verbose output
        #[arg(long, short)]
        verbose: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::New { name, with_frontend } => {
            cmd_new(name, with_frontend)?;
        }
        Commands::Build { path, release } => {
            cmd_build(path, release)?;
        }
        Commands::Package { path, with_frontend } => {
            cmd_package(path, with_frontend)?;
        }
        Commands::Validate { path } => {
            cmd_validate(path)?;
        }
        Commands::Test { path, verbose } => {
            cmd_test(path, verbose)?;
        }
    }

    Ok(())
}

fn cmd_new(name: String, with_frontend: bool) -> Result<()> {
    println!("{}", "🚀 Creating new NeoMind extension".green().bold());
    println!("{}: {}", "Name".cyan(), name);
    println!("{}: {}", "Frontend".cyan(), if with_frontend { "Yes" } else { "No" });

    // Validate extension name
    if !name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
        anyhow::bail!("Invalid extension name '{}'. Use kebab-case (e.g., my-cool-extension)", name);
    }

    // Get repository root
    let repo_root = get_repo_root()?;
    let extensions_dir = repo_root.join("extensions");
    let new_ext_dir = extensions_dir.join(&name);

    // Check if extension already exists
    if new_ext_dir.exists() {
        anyhow::bail!("Extension '{}' already exists at: {}", name, new_ext_dir.display());
    }

    // Create template
    let template_dir = repo_root.join("neomind-ext").join("templates").join("basic");
    if !template_dir.exists() {
        anyhow::bail!("Template directory not found: {}", template_dir.display());
    }

    println!("\n{}", "Creating files...".yellow());

    // Copy template
    copy_dir(&template_dir, &new_ext_dir, &name)?;

    // Update workspace Cargo.toml
    update_workspace_cargo_toml(&repo_root, &name)?;

    println!("\n{}", "✅ Extension created successfully!".green().bold());
    println!("\n{}", "Next steps:".yellow().bold());
    println!("  1. cd {}", name);
    println!("  2. {} your code", "edit".cyan());
    println!("  3. {} build the extension", "neomind-ext build".cyan());
    println!("  4. {} run tests", "neomind-ext test".cyan());
    println!("  5. {} package as .nep", "neomind-ext package".cyan());

    Ok(())
}

fn cmd_build(path: Option<String>, release: bool) -> Result<()> {
    let ext_path = get_extension_path(path)?;
    let build_type = if release { "release" } else { "debug" };

    println!("{}", format!("🔨 Building extension ({})", build_type).green().bold());

    // Check if Cargo.toml exists
    let cargo_toml = ext_path.join("Cargo.toml");
    if !cargo_toml.exists() {
        anyhow::bail!("Cargo.toml not found at: {}", ext_path.display());
    }

    // Get package name from Cargo.toml
    let package_name = extract_package_name(&cargo_toml)?;

    // Build with cargo
    println!("{}", "Running cargo build...".yellow());

    let mut cmd = Command::new("cargo");
    cmd.arg("build");

    if release {
        cmd.arg("--release");
    }

    cmd.arg("-p").arg(&package_name);

    let status = cmd
        .current_dir(&ext_path)
        .status()
        .context("Failed to run cargo build")?;

    if !status.success() {
        anyhow::bail!("Build failed with exit code: {:?}", status.code());
    }

    println!("{}", "✅ Build completed successfully!".green().bold());

    // Show output path
    let target_dir = ext_path.join("target").parallel_join(build_type);
    let lib_name = format!("lib{}.dylib", package_name);
    let lib_path = target_dir.join(&lib_name);

    if lib_path.exists() {
        println!("\n{}: {}", "Output".cyan(), lib_path.display());
    }

    Ok(())
}

fn cmd_package(path: Option<String>, with_frontend: bool) -> Result<()> {
    let ext_path = get_extension_path(path)?;

    println!("{}", "📦 Packaging extension as .nep".green().bold());

    // Get package name from Cargo.toml
    let cargo_toml = ext_path.join("Cargo.toml");
    let package_name = extract_package_name(&cargo_toml)?;

    // Find package.sh script
    let repo_root = get_repo_root()?;
    let package_script = repo_root.join("build-package.sh");

    if !package_script.exists() {
        anyhow::bail!("Package script not found: {}", package_script.display());
    }

    println!("{}", "Running package.sh...".yellow());

    // Build command - use package name
    let ext_dir_name = ext_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("Invalid extension path"))?;

    let mut cmd = Command::new("bash");
    cmd.arg(&package_script);
    cmd.arg(&ext_dir_name);

    let status = cmd
        .current_dir(&repo_root)
        .status()
        .context("Failed to run package.sh")?;

    if !status.success() {
        anyhow::bail!("Packaging failed with exit code: {:?}", status.code());
    }

    println!("{}", "✅ Package created successfully!".green().bold());

    // Show output path
    let dist_dir = repo_root.join("dist");
    if dist_dir.exists() {
        let mut entries: Vec<_> = fs::read_dir(&dist_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|ext| ext == "nep")
                    .unwrap_or(false)
            })
            .collect();

        entries.sort_by_key(|e| e.metadata().ok().and_then(|m| m.modified().ok()));

        if let Some(last_entry) = entries.last() {
            println!("\n{}: {}", "Output".cyan(), last_entry.path().display());
        }
    }

    Ok(())
}

fn cmd_validate(path: Option<String>) -> Result<()> {
    println!("{}", "✅ Validating extension".green().bold());

    let target_path = if let Some(p) = path {
        PathBuf::from(p)
    } else {
        // Current directory
        std::env::current_dir()?
    };

    // Check if it's a .nep file
    let is_nep_file = target_path
        .extension()
        .and_then(|s| s.to_str())
        .map(|ext| ext == "nep")
        .unwrap_or(false);

    if is_nep_file {
        // Validate .nep file
        validate_nep_package(&target_path)?;
    } else {
        // Validate extension directory
        validate_extension_directory(&target_path)?;
    }

    Ok(())
}

fn validate_nep_package(nep_path: &PathBuf) -> Result<()> {
    println!("{}: {}", "Validating".cyan(), nep_path.display());

    // Find test_nep.py script
    let repo_root = get_repo_root()?;
    let test_script = repo_root.join("scripts").join("test_nep.py");

    if !test_script.exists() {
        anyhow::bail!("Test script not found: {}", test_script.display());
    }

    // Run Python script
    let mut cmd = Command::new("python3");
    cmd.arg(&test_script);
    cmd.arg(nep_path);

    let status = cmd
        .status()
        .context("Failed to run test_nep.py")?;

    if !status.success() {
        anyhow::bail!("Validation failed with exit code: {:?}", status.code());
    }

    println!("{}", "✅ Package is valid!".green().bold());
    Ok(())
}

fn validate_extension_directory(ext_path: &PathBuf) -> Result<()> {
    println!("{}: {}", "Validating".cyan(), ext_path.display());

    // Check for required files
    let required_files = vec!["Cargo.toml", "src/lib.rs"];

    for file in &required_files {
        let file_path = ext_path.join(file);
        if !file_path.exists() {
            anyhow::bail!("Missing required file: {}", file);
        }
    }

    // Try to parse Cargo.toml
    let cargo_toml = ext_path.join("Cargo.toml");
    let _package_name = extract_package_name(&cargo_toml)?;

    // Check if it builds
    println!("{}", "Running cargo check...".yellow());

    let status = Command::new("cargo")
        .arg("check")
        .current_dir(ext_path)
        .status()
        .context("Failed to run cargo check")?;

    if !status.success() {
        anyhow::bail!("Cargo check failed. Extension has compilation errors.");
    }

    println!("{}", "✅ Extension is valid!".green().bold());
    Ok(())
}

fn cmd_test(path: Option<String>, verbose: bool) -> Result<()> {
    let ext_path = get_extension_path(path)?;

    println!("{}", "🧪 Running extension tests".green().bold());

    // Check if tests exist
    let tests_dir = ext_path.join("tests");
    if !tests_dir.exists() {
        println!("{} No tests directory found. Running cargo test...", "Note:".yellow());
    }

    // Run cargo test
    println!("{}", "Running cargo test...".yellow());

    let mut cmd = Command::new("cargo");
    cmd.arg("test");

    if verbose {
        cmd.arg("--").arg("--nocapture");
    }

    let status = cmd
        .current_dir(&ext_path)
        .status()
        .context("Failed to run cargo test")?;

    if !status.success() {
        anyhow::bail!("Tests failed with exit code: {:?}", status.code());
    }

    println!("{}", "✅ All tests passed!".green().bold());
    Ok(())
}

// ============================================================================
// Helper Functions
// ============================================================================

fn get_repo_root() -> Result<PathBuf> {
    let current_dir = std::env::current_dir()?;

    // Walk up the directory tree to find .git directory
    let mut dir = current_dir.as_path();
    loop {
        let git_dir = dir.join(".git");
        if git_dir.exists() {
            // Check if we're in NeoMind-Extension (look for extensions/ directory)
            let extensions_dir = dir.join("extensions");
            if extensions_dir.exists() {
                return Ok(dir.to_path_buf());
            }
        }

        let parent = match dir.parent() {
            Some(p) => p,
            None => anyhow::bail!("Could not find NeoMind-Extension repository root"),
        };

        if parent == dir {
            anyhow::bail!("Could not find NeoMind-Extension repository root");
        }

        dir = parent;
    }
}

fn get_extension_path(path: Option<String>) -> Result<PathBuf> {
    let ext_path = if let Some(p) = path {
        PathBuf::from(p)
    } else {
        // Current directory
        std::env::current_dir()?
    };

    if !ext_path.exists() {
        anyhow::bail!("Extension path does not exist: {}", ext_path.display());
    }

    Ok(ext_path)
}

fn copy_dir(src: &PathBuf, dst: &PathBuf, ext_name: &str) -> Result<()> {
    // Create destination directory
    fs::create_dir_all(dst).context("Failed to create directory")?;

    // Read source directory
    for entry in fs::read_dir(src).context("Failed to read template directory")? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let file_name = entry.file_name();
        let dst_path = dst.join(&file_name);

        if ty.is_dir() {
            // Recursively copy directory
            copy_dir(&src_path, &dst_path, ext_name)?;
        } else {
            // Read file content
            let content = fs::read_to_string(&src_path)?;

            // Replace template variables
            let content = replace_template_variables(&content, ext_name);

            // Write to destination
            fs::write(&dst_path, content).context("Failed to write file")?;

            println!("  {}", dst_path.display().to_string().dimmed());
        }
    }

    Ok(())
}

fn replace_template_variables(content: &str, ext_name: &str) -> String {
    let mut result = content.to_string();

    // Extension name transformations
    let ext_id = ext_name;
    let ext_struct_name = to_pascal_case(ext_name);
    let ext_display_name = to_display_name(ext_name);
    let ext_name_underscored = ext_name.replace('-', "_");
    let ext_package_name = format!("neomind_extension_{}", ext_name_underscored);

    // Replace variables
    result = result.replace("{{EXTENSION_NAME}}", ext_name);
    result = result.replace("{{EXTENSION_ID}}", ext_id);
    result = result.replace("{{EXTENSION_STRUCT_NAME}}", &ext_struct_name);
    result = result.replace("{{EXTENSION_DISPLAY_NAME}}", &ext_display_name);
    result = result.replace("{{EXTENSION_NAME_UNDERSCORED}}", &ext_name_underscored);
    result = result.replace("{{EXTENSION_PACKAGE_NAME}}", &ext_package_name);
    result = result.replace("{{EXTENSION_AUTHOR}}", "Your Name");
    result = result.replace("{{EXTENSION_DESCRIPTION}}", &format!("A NeoMind extension for {}", ext_display_name));

    result
}

fn to_pascal_case(s: &str) -> String {
    s.split('-')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => {
                    first.to_uppercase().collect::<String>() + chars.as_str()
                }
            }
        })
        .collect()
}

fn to_display_name(s: &str) -> String {
    s.split('-')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => {
                    first.to_uppercase().collect::<String>() + chars.as_str()
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn extract_package_name(cargo_toml: &PathBuf) -> Result<String> {
    let content = fs::read_to_string(cargo_toml)?;

    // Find name = "..." line
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("name = ") {
            let name = line
                .trim_start_matches("name = ")
                .trim_matches('"')
                .to_string();
            return Ok(name);
        }
    }

    anyhow::bail!("Could not find package name in Cargo.toml");
}

fn update_workspace_cargo_toml(repo_root: &PathBuf, ext_name: &str) -> Result<()> {
    let workspace_cargo = repo_root.join("Cargo.toml");

    if !workspace_cargo.exists() {
        return Ok(());
    }

    let ext_path = format!("extensions/{}", ext_name);
    let content = fs::read_to_string(&workspace_cargo)?;
    
    if content.contains(&ext_path) {
        return Ok(());
    }

    let lines: Vec<&str> = content.lines().collect();
    let mut result = Vec::new();
    let mut in_members = false;
    let mut inserted = false;

    for (i, line) in lines.iter().enumerate() {
        result.push(line.to_string());
        
        if line.contains("members = [") {
            in_members = true;
        } else if in_members && !inserted {
            if line.trim().starts_with('"') {
                if i + 1 < lines.len() {
                    let next_line = lines[i + 1].trim();
                    if next_line == "]" {
                        result.push(format!("    \"{}\",", ext_path));
                        inserted = true;
                    }
                }
            } else if line.trim() == "]" {
                result.push(format!("    \"{}\",", ext_path));
                inserted = true;
            }
        }
    }

    fs::write(&workspace_cargo, result.join("\n"))?;
    
    println!("  {}", "✓ Updated workspace Cargo.toml".green());

    Ok(())
}


trait PathBufExt {
    fn parallel_join(&self, other: &str) -> PathBuf;
}

impl PathBufExt for PathBuf {
    fn parallel_join(&self, other: &str) -> PathBuf {
        self.join(other)
    }
}
