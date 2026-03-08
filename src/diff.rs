//! Pure Rust parser for unified diff output.
//!
//! Parses the output of `git diff` (unified format) into structured hunks
//! that can be mapped to sign column indicators.

/// The kind of change a diff line represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    /// A line was added (exists in working copy but not in HEAD).
    Added,
    /// A line was modified (different content at the same logical position).
    Modified,
    /// A line was deleted (exists in HEAD but not in working copy).
    Deleted,
}

/// A contiguous range of changed lines in the new (working) file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    /// The kind of change.
    pub kind: ChangeKind,
    /// First line of the change in the new file (1-indexed, matching Neovim lines).
    pub start: usize,
    /// Number of lines affected in the new file. For deletions this is 0;
    /// `start` points to the line *after* which the deletion occurred.
    pub count: usize,
}

/// A single hunk header parsed from unified diff output.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RawHunk {
    old_start: usize,
    old_count: usize,
    new_start: usize,
    new_count: usize,
    lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DiffLine {
    Context,
    Added,
    Removed,
}

/// Parse a unified diff string into a list of [`Hunk`]s.
///
/// The input should be the full output of `git diff -U0` or similar.
/// Only hunks for the first file in the diff are returned; multi-file
/// diffs are supported by calling this once per file section.
#[must_use]
pub fn parse_unified_diff(diff: &str) -> Vec<Hunk> {
    let raw_hunks = parse_raw_hunks(diff);
    let mut hunks = Vec::new();

    for raw in raw_hunks {
        classify_hunk(&raw, &mut hunks);
    }

    hunks
}

/// Parse hunk headers and their content lines from unified diff output.
fn parse_raw_hunks(diff: &str) -> Vec<RawHunk> {
    let mut hunks = Vec::new();
    let mut current: Option<RawHunk> = None;

    for line in diff.lines() {
        if let Some(hunk) = parse_hunk_header(line) {
            if let Some(prev) = current.take() {
                hunks.push(prev);
            }
            current = Some(hunk);
        } else if let Some(ref mut hunk) = current {
            if let Some(first) = line.as_bytes().first() {
                match first {
                    b'+' => hunk.lines.push(DiffLine::Added),
                    b'-' => hunk.lines.push(DiffLine::Removed),
                    b' ' => hunk.lines.push(DiffLine::Context),
                    // Skip `\` (no newline at end of file) and other noise.
                    _ => {}
                }
            }
        }
    }

    if let Some(hunk) = current {
        hunks.push(hunk);
    }

    hunks
}

/// Parse a `@@ -old_start[,old_count] +new_start[,new_count] @@` header.
fn parse_hunk_header(line: &str) -> Option<RawHunk> {
    let line = line.strip_prefix("@@ ")?;
    let end = line.find(" @@")?;
    let header = &line[..end];

    let mut parts = header.split_whitespace();

    let old_part = parts.next()?.strip_prefix('-')?;
    let new_part = parts.next()?.strip_prefix('+')?;

    let (old_start, old_count) = parse_range(old_part)?;
    let (new_start, new_count) = parse_range(new_part)?;

    Some(RawHunk {
        old_start,
        old_count,
        new_start,
        new_count,
        lines: Vec::new(),
    })
}

/// Parse `start,count` or just `start` (count defaults to 1).
fn parse_range(s: &str) -> Option<(usize, usize)> {
    if let Some((start, count)) = s.split_once(',') {
        Some((start.parse().ok()?, count.parse().ok()?))
    } else {
        Some((s.parse().ok()?, 1))
    }
}

/// Classify a raw hunk into one or more sign-column [`Hunk`]s.
fn classify_hunk(raw: &RawHunk, out: &mut Vec<Hunk>) {
    // If the hunk has explicit content lines, walk them to produce
    // fine-grained add/modify/delete hunks.
    if !raw.lines.is_empty() {
        classify_from_lines(raw, out);
        return;
    }

    // Hunk header only (e.g. from `git diff -U0`): classify from counts.
    if raw.old_count == 0 && raw.new_count > 0 {
        // Pure addition.
        out.push(Hunk {
            kind: ChangeKind::Added,
            start: raw.new_start,
            count: raw.new_count,
        });
    } else if raw.old_count > 0 && raw.new_count == 0 {
        // Pure deletion.
        out.push(Hunk {
            kind: ChangeKind::Deleted,
            start: raw.new_start,
            count: 0,
        });
    } else {
        // Mixed: some lines were replaced.
        let modified = raw.old_count.min(raw.new_count);
        out.push(Hunk {
            kind: ChangeKind::Modified,
            start: raw.new_start,
            count: modified,
        });
        if raw.new_count > raw.old_count {
            out.push(Hunk {
                kind: ChangeKind::Added,
                start: raw.new_start + modified,
                count: raw.new_count - modified,
            });
        } else if raw.old_count > raw.new_count {
            out.push(Hunk {
                kind: ChangeKind::Deleted,
                start: raw.new_start + modified,
                count: 0,
            });
        }
    }
}

/// Walk content lines to emit fine-grained hunks.
fn classify_from_lines(raw: &RawHunk, out: &mut Vec<Hunk>) {
    let mut new_line = raw.new_start;
    let mut i = 0;
    let lines = &raw.lines;

    while i < lines.len() {
        match lines[i] {
            DiffLine::Context => {
                new_line += 1;
                i += 1;
            }
            DiffLine::Added => {
                // Count consecutive adds.
                let start = new_line;
                let mut count = 0;
                while i < lines.len() && lines[i] == DiffLine::Added {
                    count += 1;
                    new_line += 1;
                    i += 1;
                }
                out.push(Hunk {
                    kind: ChangeKind::Added,
                    start,
                    count,
                });
            }
            DiffLine::Removed => {
                // Count consecutive removes, then check if followed by adds (= modify).
                let mut removed = 0;
                while i < lines.len() && lines[i] == DiffLine::Removed {
                    removed += 1;
                    i += 1;
                }
                // Check if followed by adds.
                let mut added = 0;
                let add_start = new_line;
                while i < lines.len() && lines[i] == DiffLine::Added {
                    added += 1;
                    new_line += 1;
                    i += 1;
                }

                if added > 0 {
                    // Modified lines: min(removed, added) are modifications.
                    let modified = removed.min(added);
                    out.push(Hunk {
                        kind: ChangeKind::Modified,
                        start: add_start,
                        count: modified,
                    });
                    if added > removed {
                        out.push(Hunk {
                            kind: ChangeKind::Added,
                            start: add_start + modified,
                            count: added - removed,
                        });
                    } else if removed > added {
                        out.push(Hunk {
                            kind: ChangeKind::Deleted,
                            start: add_start + modified,
                            count: 0,
                        });
                    }
                } else {
                    // Pure deletion.
                    out.push(Hunk {
                        kind: ChangeKind::Deleted,
                        start: new_line,
                        count: 0,
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_diff_produces_no_hunks() {
        let hunks = parse_unified_diff("");
        assert!(hunks.is_empty());
    }

    #[test]
    fn pure_addition_u0() {
        let diff = "\
diff --git a/foo.rs b/foo.rs
index abc..def 100644
--- a/foo.rs
+++ b/foo.rs
@@ -2,0 +3,2 @@
+new line 1
+new line 2
";
        let hunks = parse_unified_diff(diff);
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].kind, ChangeKind::Added);
        assert_eq!(hunks[0].start, 3);
        assert_eq!(hunks[0].count, 2);
    }

    #[test]
    fn pure_deletion_u0() {
        let diff = "\
diff --git a/foo.rs b/foo.rs
--- a/foo.rs
+++ b/foo.rs
@@ -5,3 +4,0 @@
-removed 1
-removed 2
-removed 3
";
        let hunks = parse_unified_diff(diff);
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].kind, ChangeKind::Deleted);
        assert_eq!(hunks[0].start, 4);
        assert_eq!(hunks[0].count, 0);
    }

    #[test]
    fn modification_same_count() {
        let diff = "\
diff --git a/foo.rs b/foo.rs
--- a/foo.rs
+++ b/foo.rs
@@ -3,2 +3,2 @@
-old line 1
-old line 2
+new line 1
+new line 2
";
        let hunks = parse_unified_diff(diff);
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].kind, ChangeKind::Modified);
        assert_eq!(hunks[0].start, 3);
        assert_eq!(hunks[0].count, 2);
    }

    #[test]
    fn modification_with_extra_adds() {
        // Replace 1 line with 3 lines: 1 modified + 2 added.
        let diff = "\
diff --git a/foo.rs b/foo.rs
--- a/foo.rs
+++ b/foo.rs
@@ -5,1 +5,3 @@
-old
+new 1
+new 2
+new 3
";
        let hunks = parse_unified_diff(diff);
        assert_eq!(hunks.len(), 2);
        assert_eq!(hunks[0].kind, ChangeKind::Modified);
        assert_eq!(hunks[0].start, 5);
        assert_eq!(hunks[0].count, 1);
        assert_eq!(hunks[1].kind, ChangeKind::Added);
        assert_eq!(hunks[1].start, 6);
        assert_eq!(hunks[1].count, 2);
    }

    #[test]
    fn modification_with_extra_deletes() {
        // Replace 3 lines with 1 line: 1 modified + deletion marker.
        let diff = "\
diff --git a/foo.rs b/foo.rs
--- a/foo.rs
+++ b/foo.rs
@@ -2,3 +2,1 @@
-old 1
-old 2
-old 3
+replacement
";
        let hunks = parse_unified_diff(diff);
        assert_eq!(hunks.len(), 2);
        assert_eq!(hunks[0].kind, ChangeKind::Modified);
        assert_eq!(hunks[0].start, 2);
        assert_eq!(hunks[0].count, 1);
        assert_eq!(hunks[1].kind, ChangeKind::Deleted);
        assert_eq!(hunks[1].start, 3);
        assert_eq!(hunks[1].count, 0);
    }

    #[test]
    fn multiple_hunks() {
        let diff = "\
diff --git a/foo.rs b/foo.rs
--- a/foo.rs
+++ b/foo.rs
@@ -1,0 +1,1 @@
+header
@@ -10,1 +11,1 @@
-old
+new
@@ -20,2 +21,0 @@
-gone 1
-gone 2
";
        let hunks = parse_unified_diff(diff);
        assert_eq!(hunks.len(), 3);

        assert_eq!(hunks[0].kind, ChangeKind::Added);
        assert_eq!(hunks[0].start, 1);
        assert_eq!(hunks[0].count, 1);

        assert_eq!(hunks[1].kind, ChangeKind::Modified);
        assert_eq!(hunks[1].start, 11);
        assert_eq!(hunks[1].count, 1);

        assert_eq!(hunks[2].kind, ChangeKind::Deleted);
        assert_eq!(hunks[2].start, 21);
        assert_eq!(hunks[2].count, 0);
    }

    #[test]
    fn hunk_header_without_count() {
        // `@@ -5 +5 @@` means count=1 for both sides.
        let diff = "\
diff --git a/foo.rs b/foo.rs
--- a/foo.rs
+++ b/foo.rs
@@ -5 +5 @@
-old
+new
";
        let hunks = parse_unified_diff(diff);
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].kind, ChangeKind::Modified);
        assert_eq!(hunks[0].start, 5);
        assert_eq!(hunks[0].count, 1);
    }

    #[test]
    fn context_lines_between_changes() {
        let diff = "\
diff --git a/foo.rs b/foo.rs
--- a/foo.rs
+++ b/foo.rs
@@ -1,5 +1,6 @@
 context 1
+added
 context 2
-removed
+replaced
 context 3
";
        let hunks = parse_unified_diff(diff);
        // Should produce: Added at line 2, Modified at line 4.
        assert_eq!(hunks.len(), 2);
        assert_eq!(hunks[0].kind, ChangeKind::Added);
        assert_eq!(hunks[0].start, 2);
        assert_eq!(hunks[0].count, 1);
        assert_eq!(hunks[1].kind, ChangeKind::Modified);
        assert_eq!(hunks[1].start, 4);
        assert_eq!(hunks[1].count, 1);
    }

    #[test]
    fn no_newline_at_end_of_file_marker() {
        let diff = "\
diff --git a/foo.rs b/foo.rs
--- a/foo.rs
+++ b/foo.rs
@@ -1,1 +1,1 @@
-old
+new
\\ No newline at end of file
";
        let hunks = parse_unified_diff(diff);
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].kind, ChangeKind::Modified);
    }

    #[test]
    fn header_only_no_content_lines_addition() {
        // Simulate `-U0` output where hunk has no content lines to parse.
        let diff = "@@ -0,0 +1,5 @@\n";
        let hunks = parse_unified_diff(diff);
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].kind, ChangeKind::Added);
        assert_eq!(hunks[0].start, 1);
        assert_eq!(hunks[0].count, 5);
    }

    #[test]
    fn header_only_no_content_lines_deletion() {
        let diff = "@@ -3,4 +2,0 @@\n";
        let hunks = parse_unified_diff(diff);
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].kind, ChangeKind::Deleted);
        assert_eq!(hunks[0].start, 2);
        assert_eq!(hunks[0].count, 0);
    }

    #[test]
    fn header_only_mixed_replace() {
        let diff = "@@ -10,3 +10,5 @@\n";
        let hunks = parse_unified_diff(diff);
        // 3 old → 5 new: 3 modified + 2 added.
        assert_eq!(hunks.len(), 2);
        assert_eq!(hunks[0].kind, ChangeKind::Modified);
        assert_eq!(hunks[0].start, 10);
        assert_eq!(hunks[0].count, 3);
        assert_eq!(hunks[1].kind, ChangeKind::Added);
        assert_eq!(hunks[1].start, 13);
        assert_eq!(hunks[1].count, 2);
    }

    #[test]
    fn new_file_diff() {
        let diff = "\
diff --git a/new.rs b/new.rs
new file mode 100644
index 0000000..abc1234
--- /dev/null
+++ b/new.rs
@@ -0,0 +1,3 @@
+line 1
+line 2
+line 3
";
        let hunks = parse_unified_diff(diff);
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].kind, ChangeKind::Added);
        assert_eq!(hunks[0].start, 1);
        assert_eq!(hunks[0].count, 3);
    }

    #[test]
    fn deleted_file_diff() {
        let diff = "\
diff --git a/old.rs b/old.rs
deleted file mode 100644
index abc1234..0000000
--- a/old.rs
+++ /dev/null
@@ -1,4 +0,0 @@
-line 1
-line 2
-line 3
-line 4
";
        let hunks = parse_unified_diff(diff);
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].kind, ChangeKind::Deleted);
        assert_eq!(hunks[0].start, 0);
        assert_eq!(hunks[0].count, 0);
    }

    #[test]
    fn consecutive_add_and_delete_hunks() {
        let diff = "\
@@ -5,2 +5,0 @@
-del 1
-del 2
@@ -10,0 +8,3 @@
+add 1
+add 2
+add 3
";
        let hunks = parse_unified_diff(diff);
        assert_eq!(hunks.len(), 2);
        assert_eq!(hunks[0].kind, ChangeKind::Deleted);
        assert_eq!(hunks[0].start, 5);
        assert_eq!(hunks[1].kind, ChangeKind::Added);
        assert_eq!(hunks[1].start, 8);
        assert_eq!(hunks[1].count, 3);
    }

    #[test]
    fn parse_range_helper() {
        assert_eq!(parse_range("10,3"), Some((10, 3)));
        assert_eq!(parse_range("7"), Some((7, 1)));
        assert_eq!(parse_range("0,0"), Some((0, 0)));
        assert_eq!(parse_range(""), None);
        assert_eq!(parse_range("abc"), None);
    }
}
