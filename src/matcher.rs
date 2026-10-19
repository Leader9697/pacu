//! Pencocokan nama proses dengan pola wildcard (`*` dan `?`), tanpa membedakan huruf besar/kecil.
//! Pola yang berisi `\` atau `/` dicocokkan ke path lengkap, selain itu ke nama exe.

pub fn matches(pattern: &str, name_lc: &str, path_lc: &str) -> bool {
    let p = pattern.trim().to_lowercase();
    if p.is_empty() {
        return false;
    }
    if p.contains('\\') || p.contains('/') {
        wildcard(&p.replace('/', "\\"), path_lc)
    } else {
        wildcard(&p, name_lc)
    }
}

pub fn matches_any(patterns: &[String], name_lc: &str, path_lc: &str) -> bool {
    patterns.iter().any(|p| matches(p, name_lc, path_lc))
}

pub fn wildcard(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (None, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_basics() {
        assert!(wildcard("*.exe", "game.exe"));
        assert!(wildcard("val*nt?.exe", "valorant1.exe"));
        assert!(!wildcard("chrome.exe", "chromex.exe"));
        assert!(wildcard("*", ""));
    }

    #[test]
    fn name_and_path() {
        assert!(matches("Chrome.EXE", "chrome.exe", "c:\\x\\chrome.exe"));
        assert!(matches("*\\steamapps\\common\\*", "game.exe", "d:\\steam\\steamapps\\common\\g\\game.exe"));
        assert!(matches("*/steamapps/common/*", "game.exe", "d:\\steam\\steamapps\\common\\g\\game.exe"));
        assert!(!matches("", "a.exe", "a.exe"));
    }
}
