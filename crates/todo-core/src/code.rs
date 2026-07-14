use rand::Rng as _;

use crate::model::TodoId;

/// 사람이 부르는 짧은 코드의 문자 집합. 소문자+숫자에서 말·눈으로 헷갈리는
/// `0 1 o i l` 을 뺀 31 글자.
const ALPHABET: &[u8] = b"23456789abcdefghjkmnpqrstuvwxyz";
const CODE_LEN: usize = 4;

/// todo 를 가리키는 참조. 짧은 코드이거나 UUID 다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TodoRef {
    Code(String),
    Id(TodoId),
}

/// 무작위 4 글자 코드를 만든다. 유일성은 저장 계층(UNIQUE)이 보장하고,
/// 여기서는 문자 집합과 길이만 책임진다.
pub fn generate_code() -> String {
    let mut rng = rand::rng();
    (0..CODE_LEN)
        .map(|_| ALPHABET[rng.random_range(0..ALPHABET.len())] as char)
        .collect()
}

/// 입력을 todo 참조로 해석한다. `#` 접두사는 무시하고, 4 글자 코드는
/// 대소문자를 가리지 않고 소문자로 정규화한다. 그 밖은 UUID 로 파싱한다.
pub fn parse_ref(input: &str) -> Option<TodoRef> {
    let trimmed = input.trim();
    let trimmed = trimmed.strip_prefix('#').unwrap_or(trimmed);
    if trimmed.len() == CODE_LEN
        && trimmed
            .bytes()
            .all(|b| ALPHABET.contains(&b.to_ascii_lowercase()))
    {
        return Some(TodoRef::Code(trimmed.to_ascii_lowercase()));
    }
    trimmed.parse::<TodoId>().ok().map(TodoRef::Id)
}

#[cfg(test)]
mod tests {
    use super::{ALPHABET, CODE_LEN, TodoRef, generate_code, parse_ref};
    use crate::model::TodoId;

    #[test]
    fn generated_code_is_four_chars_from_the_alphabet() {
        for _ in 0..100 {
            let code = generate_code();
            assert_eq!(code.len(), CODE_LEN);
            assert!(
                code.bytes().all(|b| ALPHABET.contains(&b)),
                "code {code} has an out-of-set char"
            );
        }
    }

    #[test]
    fn parses_a_lowercase_code() {
        assert_eq!(parse_ref("ab3c"), Some(TodoRef::Code("ab3c".to_owned())));
    }

    #[test]
    fn parses_a_code_case_insensitively_and_strips_hash() {
        assert_eq!(parse_ref("#AB3C"), Some(TodoRef::Code("ab3c".to_owned())));
    }

    #[test]
    fn parses_a_uuid_as_id() {
        let id = TodoId::new();
        assert_eq!(parse_ref(&id.to_string()), Some(TodoRef::Id(id)));
    }

    #[test]
    fn rejects_four_chars_with_an_excluded_letter() {
        // '1' 은 집합 밖이다. 코드도 UUID 도 아니므로 참조가 아니다.
        assert_eq!(parse_ref("ab1c"), None);
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse_ref("nonsense"), None);
    }
}
