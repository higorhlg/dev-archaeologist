//! Ranks the user's tickets against one report entry by place, time and shared words.

use std::collections::{BTreeSet, HashMap};
use std::sync::OnceLock;

use regex::Regex;

use super::Candidate;

const DAY_MS: i64 = 86_400_000;
/// Score points, by kind of evidence.
const SAME_REPO: f64 = 3.0;
const OTHER_REPO: f64 = -2.0;
const KNOWN_PREFIX: f64 = 3.0;
const SAME_DAY: f64 = 1.0;
/// Above `WORDS`, so a ticket the work names beats any word overlap.
const MENTIONED: f64 = 6.0;
const WORDS: f64 = 5.0;
/// A match needs this score, this share of the title's word weight, and this lead over the next.
const MATCH_SCORE: f64 = 4.5;
const MATCH_WORDS: f64 = 0.25;
const MATCH_LEAD: f64 = 1.5;
/// Below a match, a close call goes to the AI tie-break when it reaches this score.
const CLOSE_SCORE: f64 = 3.0;
const CLOSE_LIMIT: usize = 4;

const STOPWORDS: &[&str] = &[
    "the", "and", "for", "with", "from", "into", "this", "that", "when", "not", "are", "was", "can", "all", "use", "via",
    "add", "fix", "fixes", "update", "remove", "make", "new", "feat", "chore", "refactor", "test", "tests", "wip",
    "merge", "branch", "pull", "request", "main", "master", "issue", "ticket", "should", "need", "please", "also",
    "just", "then", "there", "here", "what", "how", "why", "let", "lets", "check", "file", "files", "code",
];

/// One report entry, described by what a ticket could match.
#[derive(Debug, Clone)]
pub struct EntryEvidence {
    pub id: String,
    pub day_start_ms: i64,
    pub day_end_ms: i64,
    /// `owner/name` of the entry's GitHub repo, lowercase.
    pub repo: Option<String>,
    /// Jira prefixes seen in this project's branches and commit scopes.
    pub prefixes: BTreeSet<String>,
    /// Labels, summaries, branch, agent titles, prompts and commit subjects, joined.
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct Scored<'a> {
    pub candidate: &'a Candidate,
    pub score: f64,
    /// Share of the title's word weight that the entry also has, from 0 to 1.
    pub words: f64,
    /// The entry's text names this ticket: its Jira key, or its number in the same repo.
    pub mentioned: bool,
    pub reasons: Vec<String>,
}

#[derive(Debug)]
pub enum Verdict<'a> {
    Match(Scored<'a>),
    /// Several tickets fit about equally; best first.
    Close(Vec<Scored<'a>>),
    Nothing,
}

/// Lowercase words of 3+ letters, split at camelCase, without stopwords or numbers.
pub fn words(text: &str) -> BTreeSet<String> {
    let mut spaced = String::with_capacity(text.len());
    let mut prev_lower = false;
    for ch in text.chars() {
        if ch.is_uppercase() && prev_lower {
            spaced.push(' ');
        }
        prev_lower = ch.is_lowercase();
        spaced.push(ch);
    }
    spaced
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3 && !w.chars().all(|c| c.is_ascii_digit()) && !STOPWORDS.contains(w))
        .map(|w| if w.len() > 4 { w.strip_suffix('s').unwrap_or(w).to_string() } else { w.to_string() })
        .collect()
}

/// `#2928`, `PR 2928`, `pull request 2928`, `issue 12` in free text.
fn number_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)(?:#|\bpr\s*|\bpull request\s*|\bissues?\s*)(\d{1,6})\b").unwrap())
}

/// Ticket numbers the text names, such as `2928` from "PR 2928 status".
pub fn mentioned_numbers(text: &str) -> BTreeSet<String> {
    number_re().captures_iter(text).map(|c| c[1].to_string()).collect()
}

fn is_mentioned(c: &Candidate, e: &EntryEvidence, numbers: &BTreeSet<String>) -> bool {
    match (&c.repo, c.key.rsplit_once('#')) {
        (Some(repo), Some((_, n))) => e.repo.as_ref() == Some(repo) && numbers.contains(n),
        _ => e.text.contains(&c.key),
    }
}

/// How rare each title word is among the candidates. A word in every title counts little.
pub struct WordWeights(HashMap<String, f64>);

impl WordWeights {
    pub fn new(candidates: &[Candidate]) -> Self {
        let mut df: HashMap<String, usize> = HashMap::new();
        for c in candidates {
            for w in words(&c.title) {
                *df.entry(w).or_default() += 1;
            }
        }
        let n = candidates.len().max(1) as f64;
        Self(df.into_iter().map(|(w, d)| (w, (1.0 + n / d as f64).ln())).collect())
    }

    fn of(&self, w: &str) -> f64 {
        self.0.get(w).copied().unwrap_or(1.0)
    }
}

/// A ticket made after the day, or quiet for days before it, cannot be that day's work.
fn plausible(c: &Candidate, e: &EntryEvidence) -> bool {
    c.created_ms <= e.day_end_ms + DAY_MS && c.last_ms >= e.day_start_ms - 3 * DAY_MS
}

fn jira_prefix(key: &str) -> Option<&str> {
    key.split_once('-').map(|(p, _)| p).filter(|p| p.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()))
}

fn place_score(c: &Candidate, e: &EntryEvidence, reasons: &mut Vec<String>) -> f64 {
    match (&c.repo, &e.repo) {
        (Some(cr), Some(er)) if cr == er => {
            reasons.push("same repo".into());
            SAME_REPO
        }
        (Some(_), _) => OTHER_REPO,
        (None, _) => match jira_prefix(&c.key) {
            Some(p) if e.prefixes.contains(p) => {
                reasons.push(format!("{p} keys appear in this repo"));
                KNOWN_PREFIX
            }
            _ => 0.0,
        },
    }
}

fn words_score(c: &Candidate, entry_words: &BTreeSet<String>, weights: &WordWeights, reasons: &mut Vec<String>) -> f64 {
    let title = words(&c.title);
    let total: f64 = title.iter().map(|w| weights.of(w)).sum();
    let shared: Vec<&String> = title.iter().filter(|w| entry_words.contains(*w)).collect();
    if total <= 0.0 || shared.is_empty() {
        return 0.0;
    }
    let share = shared.iter().map(|w| weights.of(w)).sum::<f64>() / total;
    reasons.push(format!("shares {}", shared.iter().take(4).map(|w| w.as_str()).collect::<Vec<_>>().join(", ")));
    share
}

/// Scores every plausible candidate for the entry, best first.
pub fn rank<'a>(e: &EntryEvidence, candidates: &'a [Candidate], weights: &WordWeights) -> Vec<Scored<'a>> {
    let entry_words = words(&e.text);
    let numbers = mentioned_numbers(&e.text);
    let mut out: Vec<Scored> = candidates
        .iter()
        .filter(|c| plausible(c, e))
        .map(|c| {
            let mut reasons = Vec::new();
            let mut score = place_score(c, e, &mut reasons);
            let touched = (e.day_start_ms..e.day_end_ms).contains(&c.last_ms) || (e.day_start_ms..e.day_end_ms).contains(&c.created_ms);
            if touched {
                score += SAME_DAY;
                reasons.push("changed that day".into());
            }
            let mentioned = is_mentioned(c, e, &numbers);
            if mentioned {
                score += MENTIONED;
                reasons.insert(0, "named in the work".into());
            }
            let words = words_score(c, &entry_words, weights, &mut reasons);
            score += WORDS * words;
            Scored { candidate: c, score, words, mentioned, reasons }
        })
        .collect();
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    out
}

/// A clear winner is a match. Close scores with some shared words are a close call.
/// A ticket the entry neither names nor shares words with is never suggested.
pub fn judge<'a>(e: &EntryEvidence, candidates: &'a [Candidate], weights: &WordWeights) -> Verdict<'a> {
    let ranked: Vec<Scored> = rank(e, candidates, weights).into_iter().filter(|s| s.words > 0.0 || s.mentioned).collect();
    let Some(best) = ranked.first() else { return Verdict::Nothing };
    let lead = ranked.get(1).map_or(f64::INFINITY, |s| best.score - s.score);
    if best.score >= MATCH_SCORE && (best.words >= MATCH_WORDS || best.mentioned) && lead >= MATCH_LEAD {
        return Verdict::Match(best.clone());
    }
    if best.score < CLOSE_SCORE {
        return Verdict::Nothing;
    }
    let floor = best.score - 2.0;
    Verdict::Close(ranked.into_iter().filter(|s| s.score >= floor).take(CLOSE_LIMIT).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 20 * DAY_MS;

    fn jira(key: &str, title: &str) -> Candidate {
        Candidate { key: key.into(), title: title.into(), url: String::new(), tracker: "jira".into(), repo: None, created_ms: DAY - 5 * DAY_MS, last_ms: DAY + 10 * DAY_MS, epic: None }
    }

    fn gh(key: &str, repo: &str, title: &str) -> Candidate {
        Candidate { repo: Some(repo.into()), tracker: "github".into(), last_ms: DAY + 3_600_000, ..jira(key, title) }
    }

    fn entry(text: &str) -> EntryEvidence {
        EntryEvidence {
            id: "e".into(),
            day_start_ms: DAY,
            day_end_ms: DAY + DAY_MS,
            repo: Some("acme/storefront".into()),
            prefixes: ["SHOP".to_string()].into(),
            text: text.into(),
        }
    }

    #[test]
    fn words_split_camel_case_and_drop_noise() {
        let w = words("fix(SHOP-12): InvoiceHeader for EU orders #341");
        assert!(w.contains("invoice") && w.contains("header") && w.contains("order"), "{w:?}");
        assert!(!w.contains("fix") && !w.contains("341") && !w.contains("for"));
    }

    #[test]
    fn a_clear_winner_matches() {
        let c = vec![jira("SHOP-1", "Invoice tax header for EU orders"), jira("SHOP-2", "Cookie consent banner"), jira("PLAT-9", "Retry webhooks")];
        let w = WordWeights::new(&c);
        match judge(&entry("Invoice header on EU checkout orders"), &c, &w) {
            Verdict::Match(s) => assert_eq!(s.candidate.key, "SHOP-1"),
            v => panic!("expected a match, got {v:?}"),
        }
    }

    #[test]
    fn two_similar_tickets_are_a_close_call() {
        let c = vec![jira("SHOP-1", "Invoice tax header for EU orders"), jira("SHOP-2", "Invoice tax header for UK orders")];
        let w = WordWeights::new(&c);
        assert!(matches!(judge(&entry("Invoice tax header"), &c, &w), Verdict::Close(v) if v.len() == 2));
    }

    #[test]
    fn no_shared_words_means_nothing() {
        let c = vec![gh("storefront#4", "acme/storefront", "Basket totals")];
        let w = WordWeights::new(&c);
        assert!(matches!(judge(&entry("Invoice header"), &c, &w), Verdict::Nothing));
    }

    #[test]
    fn a_ticket_made_after_the_day_is_skipped() {
        let mut late = jira("SHOP-3", "Invoice header");
        late.created_ms = DAY + 3 * DAY_MS;
        assert!(rank(&entry("Invoice header"), &[late], &WordWeights::new(&[])).is_empty());
    }

    #[test]
    fn a_named_pull_request_wins() {
        let c = vec![gh("storefront#2924", "acme/storefront", "Subscription banner filter"), gh("storefront#2928", "acme/storefront", "Inverse legal text on banners")];
        let w = WordWeights::new(&c);
        match judge(&entry("PR 2928 status; subscription banner filter tweaks"), &c, &w) {
            Verdict::Match(s) => assert_eq!(s.candidate.key, "storefront#2928"),
            v => panic!("expected a match, got {v:?}"),
        }
        assert_eq!(mentioned_numbers("see #12, pull request 34 and PR56"), ["12", "34", "56"].map(String::from).into());
    }

    #[test]
    fn same_repo_beats_other_repo() {
        let c = vec![gh("storefront#4", "acme/storefront", "Basket totals rounding"), gh("admin#9", "acme/admin", "Basket totals rounding")];
        let ranked = rank(&entry("basket totals rounding"), &c, &WordWeights::new(&c));
        assert_eq!(ranked[0].candidate.key, "storefront#4");
    }
}
