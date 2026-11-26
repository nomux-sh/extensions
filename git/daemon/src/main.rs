//! The program of the daemon part of the nomux git extension.

fn main() -> Result<(), nomux_extension::RunError> {
    nomux_git::extension().run()
}
