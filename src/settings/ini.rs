//! A small UTF-8 INI reader/writer shared by preferences and Space metadata.
use super::{Settings, ThemeMode};
use std::{collections::HashSet, io, ops::Range};

struct Field {
    key: String,
    value: String,
    range: Range<usize>,
}

pub(crate) struct Document {
    text: String,
    fields: Vec<Field>,
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "설정 파일 문법이 잘못되었습니다.",
    )
}

impl Document {
    pub(crate) fn parse(text: &str) -> io::Result<Self> {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let mut fields = Vec::new();
        let mut seen = HashSet::new();
        let mut offset = 0;
        let mut section = false;
        for line in text.split_inclusive('\n') {
            let trimmed = line.trim();
            if !trimmed.is_empty() && !trimmed.starts_with(['#', ';']) {
                if trimmed.starts_with('[') {
                    let end = trimmed.find(']').ok_or_else(invalid)?;
                    let tail = trimmed[end + 1..].trim();
                    if trimmed[1..end].trim().is_empty()
                        || (!tail.is_empty() && !tail.starts_with(['#', ';']))
                    {
                        return Err(invalid());
                    }
                    section = true;
                } else {
                    let content = line.trim_end_matches(['\r', '\n']);
                    let equals = content.find('=').ok_or_else(invalid)?;
                    let key = content[..equals].trim();
                    if key.is_empty()
                        || !key
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
                    {
                        return Err(invalid());
                    }
                    let raw = &content[equals + 1..];
                    let start = equals + 1 + raw.len() - raw.trim_start().len();
                    let (value, length) = parse_value(raw.trim_start())?;
                    if !section {
                        if !seen.insert(key) {
                            return Err(invalid());
                        }
                        fields.push(Field {
                            key: key.into(),
                            value,
                            range: offset + start..offset + start + length,
                        });
                    }
                }
            }
            offset += line.len();
        }
        Ok(Self {
            text: text.into(),
            fields,
        })
    }

    pub(super) fn settings(&self) -> io::Result<Settings> {
        let mut settings = Settings::default();
        for field in &self.fields {
            match field.key.as_str() {
                "version" if &self.text[field.range.clone()] != "1" => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "지원하지 않는 설정 버전입니다.",
                    ));
                }
                "wrap" => match field.value.as_str() {
                    "true" => settings.wrap = true,
                    "false" => settings.wrap = false,
                    _ => {}
                },
                key => settings.apply_stored(key, &field.value),
            }
        }
        Ok(settings)
    }

    pub(super) fn updated(&self, settings: &Settings) -> String {
        let mode = match settings.theme {
            ThemeMode::Light => "light",
            ThemeMode::Dark => "dark",
            ThemeMode::System => "system",
        };
        let font = settings.font.replace('\\', "\\\\").replace('"', "\\\"");
        let values = [
            ("version", "1".to_owned()),
            ("theme", mode.to_owned()),
            ("font", format!("\"{font}\"")),
            ("font_size", settings.font_size.to_string()),
            ("wrap", settings.wrap.to_string()),
            ("confirm_delete", settings.confirm_delete.to_string()),
        ];
        self.updated_values(&values)
    }

    pub(crate) fn value(&self, key: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|field| field.key == key)
            .map(|field| field.value.as_str())
    }

    pub(crate) fn updated_values(&self, values: &[(&str, String)]) -> String {
        let mut text = self.text.clone();
        // Reverse replacements retain the original byte ranges and all comments.
        for field in self.fields.iter().rev() {
            if let Some((_, value)) = values.iter().find(|(key, _)| *key == field.key) {
                text.replace_range(field.range.clone(), value);
            }
        }
        let newline = if self.text.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        let mut missing = String::new();
        for (key, value) in values {
            if !self.fields.iter().any(|field| field.key == *key) {
                missing.push_str(&format!("{key} = {value}{newline}"));
            }
        }
        missing.push_str(&text);
        missing
    }
}

fn parse_value(raw: &str) -> io::Result<(String, usize)> {
    let Some(quote @ ('"' | '\'')) = raw.chars().next() else {
        let value = raw.split(['#', ';']).next().unwrap().trim_end();
        if value.contains(['\'', '"', '[', ']']) {
            return Err(invalid());
        }
        return Ok((value.into(), value.len()));
    };
    let mut value = String::new();
    let mut chars = raw.char_indices().skip(1);
    while let Some((offset, c)) = chars.next() {
        if c == quote {
            let end = offset + 1;
            let tail = raw[end..].trim();
            if !tail.is_empty() && !tail.starts_with(['#', ';']) {
                return Err(invalid());
            }
            return Ok((value, end));
        }
        if c == '\\' && quote == '"' {
            let (_, escaped) = chars.next().ok_or_else(invalid)?;
            value.push(match escaped {
                '\\' => '\\',
                '"' => '"',
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                'b' => '\u{8}',
                'f' => '\u{c}',
                'u' | 'U' => {
                    let digits = if escaped == 'u' { 4 } else { 8 };
                    let mut code = 0u32;
                    for _ in 0..digits {
                        code = (code << 4)
                            | chars
                                .next()
                                .and_then(|(_, c)| c.to_digit(16))
                                .ok_or_else(invalid)?;
                    }
                    char::from_u32(code).ok_or_else(invalid)?
                }
                _ => return Err(invalid()),
            });
        } else {
            if c.is_control() {
                return Err(invalid());
            }
            value.push(c);
        }
    }
    Err(invalid())
}
