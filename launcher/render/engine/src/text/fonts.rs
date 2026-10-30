//! The faces bundled into the binary: the same files the web launcher serves
//! (launcher/public/fonts), converted from WOFF2 to TTF. Inter is the variable font, so every
//! weight the CSS asks for (400, 500, 600, 700) draws from the same outlines as the browser's.

pub const INTER_VARIABLE: &[u8] = include_bytes!("../../../assets/fonts/Inter-Variable.ttf");
pub const FIRA_CODE_400: &[u8] = include_bytes!("../../../assets/fonts/FiraCode-Regular.ttf");
pub const FIRA_CODE_500: &[u8] = include_bytes!("../../../assets/fonts/FiraCode-Medium.ttf");
/// Arrows, math signs and a check mark (the web's Inter has them; the variable subset above does
/// not). A small static Inter cut under its own family name, so only missing glyphs fall to it.
pub const INTER_SYMBOLS: &[u8] = include_bytes!("../../../assets/fonts/InterSymbols-Regular.ttf");

pub const BUNDLED: [&[u8]; 4] = [INTER_VARIABLE, FIRA_CODE_400, FIRA_CODE_500, INTER_SYMBOLS];
