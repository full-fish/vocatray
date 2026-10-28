//! 단어장: 앱에 포함된 JSON을 읽고 검증한다 (PLAN.md §6.2).

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordBook {
    pub id: String,
    pub title: String,
    pub source: String,
    pub words: Vec<Word>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Word {
    pub word: String,
    pub phonetic: String,
    pub senses: Vec<Sense>,
    pub examples: Vec<Example>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sense {
    pub pos: String,
    pub ko: String,
    /// 이 뜻만 발음이 다를 때 (예: close 형용사 /kloʊs/). 없으면 단어의 `phonetic`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phonetic: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Example {
    pub en: String,
    pub ko: String,
}

impl Word {
    /// 중복 판단과 외운 단어 기록에 쓰는 키 — 소문자 기준 (표시는 원래 철자).
    pub fn key(&self) -> String {
        self.word.trim().to_lowercase()
    }
}

/// 앱에 포함된 단어장. 단어장을 추가하면 여기에 한 줄 더한다.
const BUNDLED: &[&str] = &[
    include_str!("../wordbooks/elementary.json"),
    include_str!("../wordbooks/middle.json"),
    include_str!("../wordbooks/high.json"),
];

pub fn bundled() -> Result<Vec<WordBook>, String> {
    BUNDLED.iter().map(|json| parse(json)).collect()
}

pub fn parse(json: &str) -> Result<WordBook, String> {
    let book: WordBook =
        serde_json::from_str(json).map_err(|e| format!("단어장 JSON을 읽을 수 없음: {e}"))?;
    validate(&book)?;
    Ok(book)
}

fn validate(book: &WordBook) -> Result<(), String> {
    if book.words.is_empty() {
        return Err(format!("{}: 단어가 없음", book.id));
    }
    let mut seen = HashSet::new();
    for word in &book.words {
        let key = word.key();
        if key.is_empty() {
            return Err(format!("{}: 빈 단어가 있음", book.id));
        }
        if !seen.insert(key) {
            return Err(format!("{}: 단어 중복 '{}'", book.id, word.word));
        }
        let senses_ok = word.senses.iter().all(|s| {
            !s.pos.trim().is_empty()
                && !s.ko.trim().is_empty()
                && s.phonetic.as_ref().is_none_or(|p| !p.trim().is_empty())
        });
        if word.senses.is_empty() || !senses_ok {
            return Err(format!("{}: '{}'의 뜻이 비어 있음", book.id, word.word));
        }
        let examples_ok = word
            .examples
            .iter()
            .all(|e| !e.en.trim().is_empty() && !e.ko.trim().is_empty());
        if word.examples.is_empty() || !examples_ok {
            return Err(format!("{}: '{}'의 예문이 비어 있음", book.id, word.word));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book_json(words: &str) -> String {
        format!(r#"{{ "id": "t", "title": "테스트", "source": "", "words": [{words}] }}"#)
    }

    const APPLE: &str = r#"{ "word": "apple", "phonetic": "", "senses": [{ "pos": "n.", "ko": "사과" }],
        "examples": [{ "en": "I like apples.", "ko": "나는 사과를 좋아한다." }] }"#;

    #[test]
    fn bundled_books_are_valid_and_ids_unique() {
        let books = bundled().expect("번들 단어장 검증 실패");
        let ids: HashSet<_> = books.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(ids.len(), books.len());
    }

    #[test]
    fn sense_phonetic_is_optional() {
        let close = r#"{ "word": "close", "phonetic": "/kloʊz/",
            "senses": [{ "pos": "v.", "ko": "닫다" }, { "pos": "adj.", "ko": "가까운", "phonetic": "/kloʊs/" }],
            "examples": [{ "en": "Close the door.", "ko": "문을 닫아라." }] }"#;
        let book = parse(&book_json(close)).unwrap();
        let senses = &book.words[0].senses;
        assert_eq!(senses[0].phonetic, None);
        assert_eq!(senses[1].phonetic.as_deref(), Some("/kloʊs/"));
    }

    #[test]
    fn bundled_books_match_curriculum_counts() {
        let books = bundled().unwrap();
        let counts: Vec<_> = books.iter().map(|b| (b.id.as_str(), b.words.len())).collect();
        assert_eq!(counts, [("elementary", 800), ("middle", 1200), ("high", 1000)]);
    }

    #[test]
    fn rejects_duplicate_words_ignoring_case() {
        let upper = APPLE.replace("\"apple\"", "\"Apple\"");
        let err = parse(&book_json(&format!("{APPLE}, {upper}"))).unwrap_err();
        assert!(err.contains("중복"), "{err}");
    }

    #[test]
    fn rejects_word_without_example() {
        let no_example = APPLE.replace(
            r#"[{ "en": "I like apples.", "ko": "나는 사과를 좋아한다." }]"#,
            "[]",
        );
        let err = parse(&book_json(&no_example)).unwrap_err();
        assert!(err.contains("예문"), "{err}");
    }

    #[test]
    fn rejects_word_without_sense() {
        let no_sense = APPLE.replace(r#"[{ "pos": "n.", "ko": "사과" }]"#, "[]");
        let err = parse(&book_json(&no_sense)).unwrap_err();
        assert!(err.contains("뜻"), "{err}");
    }
}
