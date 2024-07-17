use std::process::Command;

// Checks that the risc0 toolchain and native toolchain use the same rustc version
fn main() -> Result<(), anyhow::Error> {
    println!("cargo::rerun-if-env-changed=SKIP_GUEST_BUILD");

    // Skip the check if we aren't building any guest code
    if std::env::var("SKIP_GUEST_BUILD").is_ok() {
        return Ok(());
    }
    // Outputs a string formatted like: rustc 1.75.0-dev
    let risc0_cmd_output = Command::new("cargo")
        .env("RUSTUP_TOOLCHAIN", "risc0")
        .arg("version")
        .output()
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;

    if !risc0_cmd_output.status.success() {
        anyhow::bail!("Failed to get risc0 rustc version. Is risc0 installed? If not you can install it with the `cargo-risczero` tool. Try running `cargo risczero install`, Output: {:?}", risc0_cmd_output);
    }

    // Outputs a string formatted like: cargo 1.75.0
    let native_version_cmd = Command::new("cargo")
        .arg("version")
        .output()
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    if !native_version_cmd.status.success() {
        anyhow::bail!(
            "Failed to get native cargo version: {:?}",
            native_version_cmd
        );
    }

    let risc0_rustc_version =
        parse_version_string(&String::from_utf8_lossy(&risc0_cmd_output.stdout))?;
    let native_rustc_version =
        parse_version_string(&String::from_utf8_lossy(&native_version_cmd.stdout))?;

    if risc0_rustc_version != native_rustc_version {
        anyhow::bail!(
			"Risc0 rustc version {} does not match native rustc version {}. Please \
            update your Risc0 toolchain or use a rust-toolchain.toml file to force your \
            native compiler to the correct version.\n\n   To install a specific version of the Risc0 \
            rust toolchain, use the command `cargo risczero install --version {{tag}}`.\n You can find a \
            list of available versions at https://github.com/risc0/rust/releases.\n",
			risc0_rustc_version,
			native_rustc_version
		);
    }
    Ok(())
}

fn parse_version_string(string: &str) -> Result<String, anyhow::Error> {
    let version = string
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("Failed to parse version string"))?
        .split('-')
        .next()
        .unwrap();
    Ok(version.to_string())
}
