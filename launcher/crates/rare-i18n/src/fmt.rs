//! Dates, clock times and decimal numbers the way each language writes them (the catalogs hold
//! words; these hold the order, the month names and the decimal mark).

use crate::{lang, Lang};

const EN_MON: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const ES_MON: [&str; 12] = ["ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sept", "oct", "nov", "dic"];
const PT_MON: [&str; 12] = ["jan.", "fev.", "mar.", "abr.", "mai.", "jun.", "jul.", "ago.", "set.", "out.", "nov.", "dez."];
const RU_MON: [&str; 12] = ["янв.", "февр.", "мар.", "апр.", "мая", "июн.", "июл.", "авг.", "сент.", "окт.", "нояб.", "дек."];

/// A number written with a decimal point ("12.4") in the language's own decimal mark: a comma in
/// Russian, Spanish and Portuguese ("12,4"), a point in the others.
pub fn decimal(s: &str) -> String {
    decimal_in(lang(), s)
}

pub fn decimal_in(l: Lang, s: &str) -> String {
    match l {
        Lang::Ru | Lang::Es | Lang::PtBr => s.replace('.', ","),
        Lang::En | Lang::Zh | Lang::Ja | Lang::Ko => s.to_string(),
    }
}

/// Russian month names standing alone (a chart axis, a heading): the nominative, where a date's
/// "27 мая" takes the genitive. None for languages where the two are the same.
pub fn month_standalone(month: u32) -> Option<&'static str> {
    (lang() == Lang::Ru).then(|| mon(&RU_NOM, month))
}

const RU_NOM: [&str; 12] = ["янв.", "февр.", "март", "апр.", "май", "июнь", "июль", "авг.", "сент.", "окт.", "нояб.", "дек."];

/// Month and year: "Jan 2027", "2027年1月", "2027년 1월", "янв. 2027 г.", "ene 2027", "jan. de 2027".
pub fn month_year(year: i32, month: u32) -> String {
    month_year_in(lang(), year, month)
}

pub fn month_year_in(l: Lang, year: i32, month: u32) -> String {
    match l {
        Lang::En => format!("{} {year}", mon(&EN_MON, month)),
        Lang::Zh | Lang::Ja => format!("{year}年{month}月"),
        Lang::Ko => format!("{year}년 {month}월"),
        Lang::Ru => format!("{} {year} г.", mon(&RU_NOM, month)),
        Lang::Es => format!("{} {year}", mon(&ES_MON, month)),
        Lang::PtBr => format!("{} de {year}", mon(&PT_MON, month)),
    }
}

fn mon(names: &[&'static str; 12], month: u32) -> &'static str {
    names[(month.clamp(1, 12) - 1) as usize]
}

/// A clock time: "3:07 PM" (English), "오후 3:07" (Korean), "15:07" (the others).
pub fn time_hm(h: u32, m: u32) -> String {
    time_hm_in(lang(), h, m)
}

pub fn time_hm_in(l: Lang, h: u32, m: u32) -> String {
    let h12 = if h % 12 == 0 { 12 } else { h % 12 };
    match l {
        Lang::En => format!("{h12}:{m:02} {}", if h < 12 { "AM" } else { "PM" }),
        Lang::Ko => format!("{} {h12}:{m:02}", if h < 12 { "오전" } else { "오후" }),
        _ => format!("{h}:{m:02}"),
    }
}

/// Month (1-12) and day: "Oct 27", "10月27日", "10월 27일", "27 окт.", "27 oct", "27 de out.".
pub fn date_md(month: u32, day: u32) -> String {
    date_md_in(lang(), month, day)
}

pub fn date_md_in(l: Lang, month: u32, day: u32) -> String {
    match l {
        Lang::En => format!("{} {day}", mon(&EN_MON, month)),
        Lang::Zh | Lang::Ja => format!("{month}月{day}日"),
        Lang::Ko => format!("{month}월 {day}일"),
        Lang::Ru => format!("{day} {}", mon(&RU_MON, month)),
        Lang::Es => format!("{day} {}", mon(&ES_MON, month)),
        Lang::PtBr => format!("{day} de {}", mon(&PT_MON, month)),
    }
}

/// A full date: "Oct 27, 2026", "2026年10月27日", "2026년 10월 27일", "27 окт. 2026 г.",
/// "27 oct 2026", "27 de out. de 2026".
pub fn date_ymd(year: i32, month: u32, day: u32) -> String {
    date_ymd_in(lang(), year, month, day)
}

pub fn date_ymd_in(l: Lang, year: i32, month: u32, day: u32) -> String {
    match l {
        Lang::En => format!("{} {day}, {year}", mon(&EN_MON, month)),
        Lang::Zh | Lang::Ja => format!("{year}年{month}月{day}日"),
        Lang::Ko => format!("{year}년 {month}월 {day}일"),
        Lang::Ru => format!("{day} {} {year} г.", mon(&RU_MON, month)),
        Lang::Es => format!("{day} {} {year}", mon(&ES_MON, month)),
        Lang::PtBr => format!("{day} de {} de {year}", mon(&PT_MON, month)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forms() {
        assert_eq!(time_hm_in(Lang::En, 15, 7), "3:07 PM");
        assert_eq!(time_hm_in(Lang::En, 0, 5), "12:05 AM");
        assert_eq!(time_hm_in(Lang::Ko, 15, 7), "오후 3:07");
        assert_eq!(time_hm_in(Lang::Ja, 15, 7), "15:07");
        assert_eq!(time_hm_in(Lang::Ru, 9, 41), "9:41");
        assert_eq!(date_md_in(Lang::En, 10, 27), "Oct 27");
        assert_eq!(date_md_in(Lang::Zh, 10, 27), "10月27日");
        assert_eq!(date_ymd_in(Lang::En, 2026, 10, 27), "Oct 27, 2026");
        assert_eq!(date_ymd_in(Lang::Ru, 2026, 10, 27), "27 окт. 2026 г.");
        assert_eq!(date_ymd_in(Lang::PtBr, 2026, 10, 27), "27 de out. de 2026");
        assert_eq!(date_ymd_in(Lang::Ko, 2026, 10, 27), "2026년 10월 27일");
        assert_eq!(month_year_in(Lang::En, 2027, 1), "Jan 2027");
        assert_eq!(month_year_in(Lang::Ru, 2027, 5), "май 2027 г.");
        assert_eq!(month_year_in(Lang::Ja, 2027, 5), "2027年5月");
    }

    #[test]
    fn decimal_marks() {
        assert_eq!(decimal_in(Lang::En, "23.7"), "23.7");
        assert_eq!(decimal_in(Lang::Ru, "23.7"), "23,7");
        assert_eq!(decimal_in(Lang::Es, "1.5"), "1,5");
        assert_eq!(decimal_in(Lang::PtBr, "0.4"), "0,4");
        assert_eq!(decimal_in(Lang::Ja, "23.7"), "23.7");
        // Whole numbers are left as they are.
        assert_eq!(decimal_in(Lang::Ru, "512"), "512");
    }
}
