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
    /// The queries that came back with nothing. This is the work list for
    /// improving the ranking, and it costs nothing to collect.
    pub empty_queries: Vec<String>,
    /// Oldest and newest timestamps in the window, for the header.
    pub first: Option<String>,
    pub last: Option<String>,
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
pub fn collect(conn: &Connection, cutoff: Option<&str>) -> Result<Stats> {
    let mut st = conn.prepare(
        "select ts, query, returned_ids, budget_used, followed_up, followed_up_id
           from search_log
          where ?1 is null or ts >= ?1
          order by ts",
    )?;
    let rows = st.query_map([cutoff], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, i64>(4)?,
            r.get::<_, Option<i64>>(5)?,
        ))
    })?;

    let mut s = Stats::default();
    let mut budgets: Vec<u64> = Vec::new();
    let mut weeks: Vec<(String, usize, usize)> = Vec::new();

    for row in rows {
        let (ts, query, returned, budget, followed, opened) = row?;
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
