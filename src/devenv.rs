//! Doğrulama kancaları için ortam değişkeni okuyucu. Yalnızca `devtools` özelliğiyle derlenen sürümde değişken okur;
//! normal sürümde her zaman "yok" döner (derleyici ilgili dalları siler, çalışma anında sistem çağrısı yapılmaz).

#[cfg(feature = "devtools")]
pub fn dev_var(name: &str) -> Result<String, std::env::VarError> {
    std::env::var(name)
}

#[cfg(not(feature = "devtools"))]
#[inline(always)]
pub fn dev_var(_: &str) -> Result<String, std::env::VarError> {
    Err(std::env::VarError::NotPresent)
}
