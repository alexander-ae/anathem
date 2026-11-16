//! What the search log says about whether this has been worth having.
//!
//! The question "is this earning its place after a month" had no answer with
//! numbers in it, and the data to answer it was already being written. This
//! reads it back. Nothing here runs during a search: it is a report over
//! rows the MCP server wrote at the time.
//!
//! The discipline this owes is to claim no more than the log holds. It is
//! **not** recall: you never learn whether the right memory existed and was
//! not returned, and a search nobody followed up can mean three different
//! things — nothing relevant existed, the excerpt was enough, or the answer
//! was there and ranked too low. The one signal close to ground truth is the
//! position of a memory somebody opened, because opening it says it was
//! relevant and the position says where the ranking put it.

use crate::recent;
use anyhow::Result;
use rusqlite::Connection;

#[derive(Debug, Default)]
pub struct Stats {
    pub searches: usize,
    /// Searches that returned nothing at all.
    pub empty: usize,
    /// Searches after which the agent asked for a full body.
    pub followed_up: usize,
    /// Where the opened memory sat, 1-based. Only from rows that recorded
    /// *which* memory was opened; see `a_follow_up_whose_id_was_never_recorded`.
    pub ranks: Vec<usize>,
    pub tokens_total: u64,
    pub tokens_p50: u64,
    /// `(week starting, searches, follow-ups)`, oldest first.
    pub by_week: Vec<(String, usize, usize)>,
    /// The same counts broken down by whatever `--by` asked for. Empty when
    /// nobody asked, which is what keeps the default report unchanged.
    pub by_group: Vec<GroupRow>,
    /// The queries that came back with nothing. This is the work list for
    /// improving the ranking, and it costs nothing to collect.
    pub empty_queries: Vec<String>,
    /// Oldest and newest timestamps in the window, for the header.
    pub first: Option<String>,
    pub last: Option<String>,
}

/// One row of `mem stats --by project` or `--by agent`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupRow {
    /// The project name, the agent name, or the placeholder for rows that
    /// have neither.
    pub name: String,
    pub searches: usize,
    pub followed_up: usize,
    pub empty: usize,
    pub tokens_p50: u64,
}

/// What the breakdown is grouped by. Two values and no more: a key that is
/// silently ignored is a filter that lies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum By {
    Project,
    Agent,
    /// The directory the server was running in, which is what decides the
    /// project. It answers the question `--by project` cannot: whether a
    /// search with no project was one about nothing, or one made from a
    /// session opened in the folder that holds every repository.
    Dir,
}

impl By {
    pub fn parse(text: &str) -> Option<By> {
        match text {
            "project" => Some(By::Project),
            "agent" => Some(By::Agent),
            "dir" => Some(By::Dir),
            _ => None,
        }
    }

    fn column(self) -> &'static str {
        match self {
            By::Project => "project",
            By::Agent => "agent",
            By::Dir => "cwd",
        }
    }

    /// What a row with nothing in that column is called. It is said rather
    /// than left blank: "no project" is information about the search, and an
    /// empty cell reads as a bug.
    fn unnamed(self) -> &'static str {
        match self {
            By::Project => "(no project)",
            By::Agent => "(no agent)",
            By::Dir => "(no directory)",
        }
    }

    /// Directory paths are long and the part that says which project this is
    /// lives at the end, so the table shows the last component. Two sessions
    /// in `/a/x` and `/b/x` collapse into one row, which is the trade — and
    /// they are the same project name by any other measure.
    fn label(self, raw: Option<String>) -> Option<String> {
        let raw = raw.filter(|g| !g.trim().is_empty())?;
        if self != By::Dir {
            return Some(raw);
        }
        std::path::Path::new(&raw)
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string)
            .or(Some(raw))
    }
}

impl Stats {
    /// The share of searches the agent acted on. Not a quality figure: see
    /// the module docs for the three things a missing follow-up can mean.
    pub fn answer_rate(&self) -> f64 {
        if self.searches == 0 {
            0.0
        } else {
            self.followed_up as f64 / self.searches as f64
        }
    }

    pub fn median_rank(&self) -> Option<usize> {
        if self.ranks.is_empty() {
            return None;
        }
        let mut v = self.ranks.clone();
        v.sort_unstable();
        Some(v[v.len() / 2])
    }
}

/// Reads the log. `cutoff` is an RFC3339 instant; rows older than it are
/// left out.
pub fn collect(conn: &Connection, cutoff: Option<&str>, by: Option<By>) -> Result<Stats> {
    // A column name cannot be a bound parameter, so it is written into the
    // SQL — but it comes from the enum and never from the command line, which
    // is what keeps that honest.
    let group_col = by.map(By::column).unwrap_or("null");
    let sql = format!(
        "select ts, query, returned_ids, budget_used, followed_up, followed_up_id, {group_col}
           from search_log
          where ?1 is null or ts >= ?1
          order by ts"
    );
    let mut st = conn.prepare(&sql)?;
    let rows = st.query_map([cutoff], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, i64>(4)?,
            r.get::<_, Option<i64>>(5)?,
            r.get::<_, Option<String>>(6)?,
        ))
    })?;

    let mut s = Stats::default();
    let mut budgets: Vec<u64> = Vec::new();
    let mut weeks: Vec<(String, usize, usize)> = Vec::new();
    // name -> (searches, answered, empty, tokens)
    let mut groups: std::collections::BTreeMap<String, (usize, usize, usize, Vec<u64>)> =
        std::collections::BTreeMap::new();

    for row in rows {
        let (ts, query, returned, budget, followed, opened, group) = row?;
        s.searches += 1;
        if s.first.is_none() {
            s.first = Some(ts.clone());
        }
        s.last = Some(ts.clone());

        let ids: Vec<&str> = returned
            .split(',')
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .collect();
        if ids.is_empty() {
            s.empty += 1;
            s.empty_queries.push(query);
        }
        if followed != 0 {
            s.followed_up += 1;
        }
        // Only a recorded id gives a rank. A row that says one was opened
        // without saying which cannot be placed, and guessing would invent
        // the very number this is supposed to measure.
        if let Some(id) = opened {
            let needle = id.to_string();
            if let Some(pos) = ids.iter().position(|x| *x == needle) {
                s.ranks.push(pos + 1);
            }
        }

        budgets.push(budget.max(0) as u64);
        s.tokens_total += budget.max(0) as u64;

        if let Some(by) = by {
            let name = by.label(group).unwrap_or_else(|| by.unnamed().to_string());
            let slot = groups.entry(name).or_insert((0, 0, 0, Vec::new()));
            slot.0 += 1;
            slot.1 += (followed != 0) as usize;
            slot.2 += ids.is_empty() as usize;
            slot.3.push(budget.max(0) as u64);
        }

        let week = week_of(&ts);
        match weeks.last_mut() {
            Some((w, n, f)) if *w == week => {
                *n += 1;
                *f += (followed != 0) as usize;
            }
            _ => weeks.push((week, 1, (followed != 0) as usize)),
        }
    }

    budgets.sort_unstable();
    s.tokens_p50 = budgets.get(budgets.len() / 2).copied().unwrap_or(0);
    s.by_week = weeks;
    s.by_group = groups
        .into_iter()
        .map(|(name, (searches, followed_up, empty, mut toks))| {
            toks.sort_unstable();
            let p50 = toks.get(toks.len() / 2).copied().unwrap_or(0);
            GroupRow { name, searches, followed_up, empty, tokens_p50: p50 }
        })
        .collect();
    Ok(s)
}

/// The Monday of the week a timestamp falls in, as `YYYY-MM-DD`. Falls back
/// to the date itself when the timestamp cannot be parsed, so one odd row
/// does not take the whole report down.
fn week_of(ts: &str) -> String {
    match chrono::DateTime::parse_from_rfc3339(ts) {
        Ok(d) => {
            let date = d.date_naive();
            let back = date.format("%u").to_string().parse::<i64>().unwrap_or(1) - 1;
            (date - chrono::Duration::days(back)).format("%Y-%m-%d").to_string()
        }
        Err(_) => ts.chars().take(10).collect(),
    }
}

/// The cutoff a window like `30d` means, or `None` for no bound.
pub fn cutoff_for(window: Option<&str>) -> Result<Option<String>> {
    match window {
        None => Ok(None),
        Some(w) => Ok(Some(recent::since(recent::duration_in_seconds(w)?))),
    }
}
