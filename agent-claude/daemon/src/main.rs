//! The program of the daemon part of the nomux adapter extension for Claude
//! Code.

fn main() -> Result<(), nomux_extension::RunError> {
    nomux_agent_claude::extension().run()
}
