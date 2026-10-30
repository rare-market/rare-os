//! A server-sent events parser, line by line, as the HTML spec's "interpreting an event stream":
//! `field: value` lines (one leading space dropped), `data` lines joined with newlines, `:` comments
//! ignored, a blank line dispatches. The last event id carries over between events, as
//! EventSource's `lastEventId` does.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SseEvent {
    /// The event type (`message` when the frame names none).
    pub event: String,
    pub data: String,
    /// The last event id seen on this stream, this frame's own or an earlier one.
    pub last_id: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct SseParser {
    event: String,
    data: String,
    has_data: bool,
    last_id: Option<String>,
    retry: Option<u64>,
    first: bool,
}

impl SseParser {
    pub fn new() -> Self {
        SseParser { first: true, ..Default::default() }
    }

    /// The reconnection time the server asked for (`retry:`), ms.
    pub fn retry(&self) -> Option<u64> {
        self.retry
    }

    pub fn last_id(&self) -> Option<&str> {
        self.last_id.as_deref()
    }

    /// Feeds one line (without its line ending). A blank line may complete an event.
    pub fn line(&mut self, line: &str) -> Option<SseEvent> {
        let mut line = line.strip_suffix('\r').unwrap_or(line);
        if self.first {
            self.first = false;
            line = line.strip_prefix('\u{feff}').unwrap_or(line);
        }
        if line.is_empty() {
            return self.dispatch();
        }
        if line.starts_with(':') {
            return None;
        }
        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
            None => (line, ""),
        };
        match field {
            "event" => self.event = value.to_string(),
            "data" => {
                self.data.push_str(value);
                self.data.push('\n');
                self.has_data = true;
            }
            "id" if !value.contains('\0') => self.last_id = (!value.is_empty()).then(|| value.to_string()),
            "retry" if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) => self.retry = value.parse().ok(),
            _ => {}
        }
        None
    }

    fn dispatch(&mut self) -> Option<SseEvent> {
        let event = std::mem::take(&mut self.event);
        let mut data = std::mem::take(&mut self.data);
        if !std::mem::take(&mut self.has_data) {
            return None;
        }
        if data.ends_with('\n') {
            data.pop();
        }
        Some(SseEvent { event: if event.is_empty() { "message".into() } else { event }, data, last_id: self.last_id.clone() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(lines: &[&str]) -> (Vec<SseEvent>, SseParser) {
        let mut p = SseParser::new();
        let out = lines.iter().filter_map(|l| p.line(l)).collect();
        (out, p)
    }

    #[test]
    fn parses_frames_comments_and_retry() {
        let (ev, p) = feed(&["retry: 3000", "", "event: hello", "data: {\"now\":1}", "", ": hb", "", "id: 42", "event: notice", "data: {\"a\":", "data: 1}", ""]);
        assert_eq!(p.retry(), Some(3000));
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0], SseEvent { event: "hello".into(), data: "{\"now\":1}".into(), last_id: None });
        assert_eq!(ev[1], SseEvent { event: "notice".into(), data: "{\"a\":\n1}".into(), last_id: Some("42".into()) });
    }

    #[test]
    fn keeps_the_last_id_and_defaults_the_type() {
        let (ev, _) = feed(&["\u{feff}id: 7", "data:x", "", "data: y\r", ""]);
        assert_eq!(ev[0].event, "message");
        assert_eq!(ev[0].data, "x");
        assert_eq!(ev[1].data, "y");
        assert_eq!(ev[1].last_id.as_deref(), Some("7"));
    }

    #[test]
    fn an_event_without_data_is_not_dispatched() {
        let (ev, p) = feed(&["event: x", "id: 9", "", "data", ""]);
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].event, "message");
        assert_eq!(ev[0].data, "");
        assert_eq!(p.last_id(), Some("9"));
    }
}
