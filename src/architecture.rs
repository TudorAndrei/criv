use crate::{CrivError, Result};

pub(crate) fn validate_agent_authored_config(
    code_generator_configured: bool,
    docs_dir: &str,
) -> Result<()> {
    if code_generator_configured {
        return Err(CrivError::new(format!(
            "criv.toml [architecture.code] was removed; delete it and let the coding agent author element names and view titles under {docs_dir}/architecture/"
        )));
    }
    Ok(())
}
