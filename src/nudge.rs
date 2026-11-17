pub const MAX: usize = 2;

pub const MESSAGE: &str = "You stopped without doing the next step – your answer was only an announcement or was cut off. Continue the task now: call the tool in this answer instead of describing what you are going to do. If you are really finished or need my input, say so clearly in full sentences.";
const SHORT: usize = 12;

const PHRASES: &[&str] = &[
    "let me",
    "i will",
    "i'll",
    "i am going to",
    "i'm going to",
    "now i",
    "next i",
    "first i",
    "lass mich",
    "lasse mich",
    "ich schaue",
    "ich schau",
    "ich werde",
    "ich fange",
    "ich lese",
    "ich prüfe",
    "ich sehe mir",
    "ich gucke",
    "ich mache",
    "ich starte",
    "ich beginne",
    "jetzt schaue",
    "als nächstes",
    "zuerst",
];

pub fn stalled(text: &str) -> bool {
    let visible: String = text.chars().filter(|c| c.is_alphanumeric()).collect();
    visible.chars().count() < SHORT || announces(text)
}

fn announces(text: &str) -> bool {
    let Some(last) = text.lines().map(str::trim).rfind(|line| !line.is_empty()) else {
        return false;
    };
    if last.ends_with('?') || last.chars().count() > 400 || last.starts_with("```") {
        return false;
    }
    if last.ends_with(':') || last.ends_with('…') || last.ends_with("...") {
        return true;
    }
    let lower = last.to_lowercase();
    PHRASES.iter().any(|phrase| lower.contains(phrase))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_announcements() {
        assert!(announces("Ich schaue mir jetzt die system.rs an, um das zu verstehen:"));
        assert!(announces("Verstehe. Lass mich die relevanten Dateien genauer anschauen und dann die Änderungen vornehmen."));
        assert!(announces("Alles klar, ich fange jetzt an. Lass mich zuerst die wichtigsten Dateien lesen."));
        assert!(announces("Let me check the build output first."));
    }

    #[test]
    fn finds_cut_answers() {
        assert!(stalled("##"));
        assert!(stalled("Ver"));
        assert!(stalled("OK"));
        assert!(!stalled("Fertig, der Button ist jetzt drin."));
    }

    #[test]
    fn ignores_real_answers() {
        assert!(!announces("Gib mir die konkrete Aufgabe. Alles klar? 😊"));
        assert!(!announces("Fertig. Der Button ist jetzt im Installations-Tab."));
        assert!(!announces(""));
        assert!(!announces("Soll ich weitermachen?"));
    }
}
