#[cfg(feature = "devtools")]
pub fn dev_var(name: &str) -> Result<String, std::env::VarError> {
    std::env::var(name)
}

#[cfg(not(feature = "devtools"))]
#[inline(always)]
pub fn dev_var(_: &str) -> Result<String, std::env::VarError> {
    Err(std::env::VarError::NotPresent)
}
