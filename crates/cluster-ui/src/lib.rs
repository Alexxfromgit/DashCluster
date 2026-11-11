//! Slint widget library and the glue that turns dashboard layouts into widget models.

slint::include_modules!();

#[cfg(feature = "offscreen")]
pub mod offscreen;
