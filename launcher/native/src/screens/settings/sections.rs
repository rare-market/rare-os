//! The sections of Settings, in the order of the left list (screens/settings/sections.ts).

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SectionId {
    System,
    Display,
    Sound,
    Controls,
    Power,
    Network,
    Accounts,
    Devices,
    Parental,
    Storage,
    Capture,
    About,
    Emulators,
    Notifications,
    Licences,
    Developer,
    Ask,
}

pub const SECTIONS: [(SectionId, &str); 17] = [
    (SectionId::System, "System"),
    (SectionId::Display, "Display"),
    (SectionId::Sound, "Sound"),
    (SectionId::Controls, "Controls"),
    (SectionId::Power, "Power"),
    (SectionId::Network, "Network"),
    (SectionId::Accounts, "Accounts"),
    (SectionId::Devices, "Devices"),
    (SectionId::Parental, "Parental controls"),
    (SectionId::Storage, "Storage"),
    (SectionId::Capture, "Capture and streaming"),
    (SectionId::Notifications, "Notifications"),
    (SectionId::Ask, "Ask Rare"),
    (SectionId::Emulators, "Emulators"),
    (SectionId::Developer, "Developer"),
    (SectionId::About, "About"),
    (SectionId::Licences, "Credits and licences"),
];

impl SectionId {
    pub fn label(self) -> &'static str {
        SECTIONS.iter().find(|(id, _)| *id == self).map(|(_, l)| *l).unwrap_or("")
    }

    /// The id the web launcher uses in `data-section` and the jump store.
    pub fn key(self) -> &'static str {
        match self {
            SectionId::System => "system",
            SectionId::Display => "display",
            SectionId::Sound => "sound",
            SectionId::Controls => "controls",
            SectionId::Power => "power",
            SectionId::Network => "network",
            SectionId::Accounts => "accounts",
            SectionId::Devices => "devices",
            SectionId::Parental => "parental",
            SectionId::Storage => "storage",
            SectionId::Capture => "capture",
            SectionId::About => "about",
            SectionId::Emulators => "emulators",
            SectionId::Notifications => "notifications",
            SectionId::Licences => "licences",
            SectionId::Developer => "developer",
            SectionId::Ask => "ask",
        }
    }

    pub fn from_key(k: &str) -> Option<SectionId> {
        SECTIONS.iter().map(|(id, _)| *id).find(|id| id.key() == k)
    }
}

/// LT / RT: the section `step` places away (wraps).
pub fn step(from: SectionId, forward: bool) -> SectionId {
    let i = SECTIONS.iter().position(|(id, _)| *id == from).unwrap_or(0);
    let n = SECTIONS.len();
    SECTIONS[(i + if forward { 1 } else { n - 1 }) % n].0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lt_rt_wrap_around_the_list() {
        assert_eq!(step(SectionId::System, false), SectionId::Licences);
        assert_eq!(step(SectionId::Licences, true), SectionId::System);
        assert_eq!(step(SectionId::Display, true), SectionId::Sound);
    }

    #[test]
    fn keys_round_trip() {
        for (id, _) in SECTIONS {
            assert_eq!(SectionId::from_key(id.key()), Some(id));
        }
    }
}
