use chrono::NaiveDate;
use git_core::history::HistoryEntry;

#[derive(Debug, Clone, Default)]
pub struct DateFilter {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DateValidation {
    pub from_invalid: bool,
    pub to_invalid: bool,
    /// true when from > to
    pub range_inverted: bool,
}

impl DateValidation {
    pub fn any_invalid(&self) -> bool {
        self.from_invalid || self.to_invalid || self.range_inverted
    }
}

pub fn parse_date(s: &str) -> Result<Option<NaiveDate>, ()> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map(Some)
        .map_err(|_| ())
}

pub fn validate_dates(from_text: &str, to_text: &str) -> (DateFilter, DateValidation) {
    let from_result = parse_date(from_text);
    let to_result = parse_date(to_text);

    let from_invalid = from_result.is_err();
    let to_invalid = to_result.is_err();

    let from = from_result.unwrap_or(None);
    let to = to_result.unwrap_or(None);

    let range_inverted = matches!((&from, &to), (Some(f), Some(t)) if f > t);

    let validation = DateValidation {
        from_invalid,
        to_invalid,
        range_inverted,
    };

    let date_filter = if validation.any_invalid() {
        DateFilter {
            from: None,
            to: None,
        }
    } else {
        DateFilter { from, to }
    };

    (date_filter, validation)
}

pub fn apply_filter<'a>(
    entries: &'a [HistoryEntry],
    text: &str,
    date: &DateFilter,
) -> Vec<&'a HistoryEntry> {
    let needle = text.trim().to_lowercase();
    let has_text = !needle.is_empty();
    let has_date = date.from.is_some() || date.to.is_some();

    if !has_text && !has_date {
        return entries.iter().collect();
    }

    entries
        .iter()
        .filter(|e| {
            let text_match = if has_text {
                let short_hash = if e.id.len() >= 8 { &e.id[..8] } else { &e.id };
                e.message.to_lowercase().contains(&needle)
                    || short_hash.starts_with(&needle)
                    || e.author_name.to_lowercase().contains(&needle)
                    || e.author_email.to_lowercase().contains(&needle)
            } else {
                true
            };

            let date_match = if has_date {
                // timestamp is Unix seconds (author date)
                let entry_date = chrono::DateTime::from_timestamp(e.timestamp, 0)
                    .map(|dt| dt.naive_utc().date());
                match entry_date {
                    None => true,
                    Some(d) => {
                        let from_ok = date.from.map_or(true, |from| d >= from);
                        let to_ok = date.to.map_or(true, |to| d <= to);
                        from_ok && to_ok
                    }
                }
            } else {
                true
            };

            text_match && date_match
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use git_core::history::HistoryEntry;

    fn e(id: &str, msg: &str, name: &str, email: &str, ts: i64) -> HistoryEntry {
        HistoryEntry {
            id: id.into(),
            message: msg.into(),
            author_name: name.into(),
            author_email: email.into(),
            timestamp: ts,
            parent_ids: vec![],
            committer_name: None,
            committer_email: None,
            refs: vec![],
            signature_status: None,
        }
    }

    #[test]
    fn empty_filter_returns_all() {
        let entries = vec![
            e("abc12345", "fix bug", "Alice", "a@x.com", 0),
            e("def67890", "add feature", "Bob", "b@x.com", 0),
        ];
        assert_eq!(apply_filter(&entries, "", &DateFilter::default()).len(), 2);
    }

    #[test]
    fn text_filter_matches_subject() {
        let entries = vec![
            e("abc12345", "fix bug in login", "Alice", "a@x.com", 0),
            e("def67890", "add feature", "Bob", "b@x.com", 0),
        ];
        let result = apply_filter(&entries, "bug", &DateFilter::default());
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].message, "fix bug in login");
    }

    #[test]
    fn text_filter_matches_short_hash() {
        let entries = vec![
            e("abc12345def", "commit one", "Alice", "a@x.com", 0),
            e("xyz99999zzz", "commit two", "Bob", "b@x.com", 0),
        ];
        assert_eq!(
            apply_filter(&entries, "abc1", &DateFilter::default()).len(),
            1
        );
    }

    #[test]
    fn validate_dates_empty_is_valid() {
        let (filter, validation) = validate_dates("", "");
        assert!(!validation.any_invalid());
        assert!(filter.from.is_none() && filter.to.is_none());
    }

    #[test]
    fn validate_dates_inverted_range() {
        let (_, validation) = validate_dates("2026-04-10", "2026-04-01");
        assert!(validation.range_inverted && validation.any_invalid());
    }
}
