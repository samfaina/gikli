//! The diff of a changed file against HEAD: what the side-by-side view shows.

use std::fs;
use std::path::PathBuf;

use gikli_acceptance::{Sandbox, TestRepository, file_url};
use gikli_core::{
    ChangeGrouping, DiffContent, Difference, Failure, FileDiff, Gikli, LineRange, LocalReadOnly,
    Repository, TextDiff, UnchangedRegion,
};

/// The Repository of `repository`, opened as a Project.
fn opened(gikli: &Gikli, repository: &TestRepository) -> Repository {
    let project = gikli.open_project(repository.root()).expect("open project");
    project.repositories[0].clone()
}

/// The diff of the file at `path`.
fn read_diff(gikli: &Gikli, repository: &TestRepository, path: &str) -> FileDiff {
    let opened = opened(gikli, repository);
    gikli.read_diff(&opened, path).expect("read diff")
}

/// The text diff in `diff`, which must have one.
fn text_of(diff: &FileDiff) -> &TextDiff {
    match &diff.content {
        DiffContent::Text(text) => text,
        other => panic!("expected a text diff, got {other:?}"),
    }
}

/// The ranges of the hunks plain git finds between HEAD and the file at `path`, as `-old +new`.
fn hunks_by_git(repository: &TestRepository, path: &str) -> Vec<String> {
    repository
        .git(&[
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--unified=0",
            "--inter-hunk-context=0",
            "HEAD",
            "--",
            path,
        ])
        .lines()
        .filter_map(|line| line.strip_prefix("@@ "))
        .filter_map(|line| line.split(" @@").next())
        .map(str::to_owned)
        .collect()
}

/// Whether plain git takes the difference between HEAD and the file at `path` as binary.
fn binary_by_git(repository: &TestRepository, path: &str) -> bool {
    repository
        .git(&["diff", "--numstat", "HEAD", "--", path])
        .starts_with("-\t-\t")
}

/// The lines from `start` up to `end`, counted from 0.
fn lines(start: usize, end: usize) -> LineRange {
    LineRange { start, end }
}

fn difference(head: LineRange, local: LineRange) -> Difference {
    Difference { head, local }
}

#[test]
fn an_edited_file_shows_both_versions_and_where_they_differ() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("notes.txt", "one\ntwo\nthree\nfour\n");
    repository.commit_all("Initial commit");
    repository.write("notes.txt", "one\nTWO\nthree\nfour\nfive\n");

    let diff = read_diff(&sandbox.gikli(), &repository, "notes.txt");

    assert_eq!(diff.path, PathBuf::from("notes.txt"));
    let text = text_of(&diff);
    assert_eq!(text.head.as_deref(), Some("one\ntwo\nthree\nfour\n"));
    assert_eq!(text.local.as_deref(), Some("one\nTWO\nthree\nfour\nfive\n"));
    assert_eq!(
        text.differences,
        vec![
            difference(lines(1, 2), lines(1, 2)),
            difference(lines(4, 4), lines(4, 5)),
        ]
    );
    assert_eq!(
        repository.git(&["show", "HEAD:notes.txt"]),
        "one\ntwo\nthree\nfour\n"
    );
}

#[test]
fn a_new_file_has_no_head_side_and_every_line_differs() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write(
        "README.md",
        "hello
",
    );
    repository.commit_all("Initial commit");
    repository.write(
        "new.txt", "one
two
",
    );

    let diff = read_diff(&sandbox.gikli(), &repository, "new.txt");

    let text = text_of(&diff);
    assert_eq!(text.head, None);
    assert_eq!(
        text.local.as_deref(),
        Some(
            "one
two
"
        )
    );
    assert_eq!(text.differences, vec![difference(lines(0, 0), lines(0, 2))]);
    assert_eq!(repository.status_by_git(), vec!["?? new.txt"]);
}

#[test]
fn a_file_added_in_a_repository_with_no_commits_has_no_head_side() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write(
        "first.txt",
        "one
",
    );
    repository.git(&["add", "first.txt"]);

    let diff = read_diff(&sandbox.gikli(), &repository, "first.txt");

    let text = text_of(&diff);
    assert_eq!(text.head, None);
    assert_eq!(
        text.local.as_deref(),
        Some(
            "one
"
        )
    );
    assert_eq!(text.differences, vec![difference(lines(0, 0), lines(0, 1))]);
    assert!(!repository.git_succeeds(&["rev-parse", "--verify", "--quiet", "HEAD"]));
}

#[test]
fn a_deleted_file_has_no_local_side_and_every_line_differs() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write(
        "gone.txt", "one
two
",
    );
    repository.commit_all("Initial commit");
    repository.delete("gone.txt");

    let diff = read_diff(&sandbox.gikli(), &repository, "gone.txt");

    let text = text_of(&diff);
    assert_eq!(
        text.head.as_deref(),
        Some(
            "one
two
"
        )
    );
    assert_eq!(text.local, None);
    assert_eq!(text.differences, vec![difference(lines(0, 2), lines(0, 0))]);
    assert_eq!(repository.status_by_git(), vec![" D gone.txt"]);
}

#[test]
fn a_file_removed_from_git_too_has_no_local_side() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write(
        "gone.txt", "one
",
    );
    repository.commit_all("Initial commit");
    repository.git(&["rm", "--quiet", "gone.txt"]);

    let diff = read_diff(&sandbox.gikli(), &repository, "gone.txt");

    let text = text_of(&diff);
    assert_eq!(text.local, None);
    assert_eq!(text.differences, vec![difference(lines(0, 1), lines(0, 0))]);
    assert_eq!(repository.status_by_git(), vec!["D  gone.txt"]);
}

#[test]
fn a_renamed_file_is_compared_with_its_original_path_in_head() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    let contents = "one
two
three
four
five
six
";
    repository.write("old.txt", contents);
    repository.commit_all("Initial commit");
    repository.git(&["mv", "old.txt", "new.txt"]);
    repository.write(
        "new.txt",
        "one
two
three
four
five
SIX
",
    );

    let diff = read_diff(&sandbox.gikli(), &repository, "new.txt");

    assert_eq!(diff.original_path, Some(PathBuf::from("old.txt")));
    let text = text_of(&diff);
    assert_eq!(text.head.as_deref(), Some(contents));
    assert_eq!(text.differences, vec![difference(lines(5, 6), lines(5, 6))]);
    assert_eq!(repository.git(&["show", "HEAD:old.txt"]), contents);
}

/// A rename so far from the original that git would not find it again by comparing them: what
/// matters is what the changes view says it was, so the sides still line up.
#[test]
fn a_renamed_file_rewritten_on_disk_is_still_compared_with_its_original_path() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write(
        "old.txt",
        "one
two
three
",
    );
    repository.commit_all("Initial commit");
    repository.git(&["mv", "old.txt", "new.txt"]);
    repository.write(
        "new.txt",
        "alpha
beta
",
    );

    let diff = read_diff(&sandbox.gikli(), &repository, "new.txt");

    let text = text_of(&diff);
    assert_eq!(
        text.head.as_deref(),
        Some(
            "one
two
three
"
        )
    );
    assert_eq!(text.differences, vec![difference(lines(0, 3), lines(0, 2))]);
    assert_eq!(
        repository.git(&["show", "HEAD:old.txt"]),
        "one\ntwo\nthree\n"
    );
}

#[test]
fn a_path_with_spaces_unicode_and_glob_characters_is_taken_literally() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    let path = "carpeta ñ/año [1].txt";
    repository.write(
        path, "uno
",
    );
    repository.write(
        "carpeta ñ/año 1.txt",
        "otro
",
    );
    repository.commit_all("Initial commit");
    repository.write(
        path, "dos
",
    );

    let diff = read_diff(&sandbox.gikli(), &repository, path);

    let text = text_of(&diff);
    assert_eq!(
        text.head.as_deref(),
        Some(
            "uno
"
        )
    );
    assert_eq!(
        text.local.as_deref(),
        Some(
            "dos
"
        )
    );
    assert_eq!(text.differences, vec![difference(lines(0, 1), lines(0, 1))]);
    assert_eq!(repository.git(&["show", &format!("HEAD:{path}")]), "uno\n");
}

#[test]
fn a_last_line_without_a_line_ending_counts_as_a_line() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write(
        "notes.txt",
        "one
two",
    );
    repository.commit_all("Initial commit");
    repository.write(
        "notes.txt",
        "one
two
",
    );

    let diff = read_diff(&sandbox.gikli(), &repository, "notes.txt");

    assert_eq!(
        text_of(&diff).differences,
        vec![difference(lines(1, 2), lines(1, 2))]
    );
    assert_eq!(hunks_by_git(&repository, "notes.txt"), vec!["-2 +2"]);
}

/// git's settings for how a diff looks, or what makes it, would change what gikli reads.
#[test]
fn the_users_diff_settings_change_nothing() {
    let sandbox = Sandbox::new();
    sandbox.configure(concat!(
        "[diff]
	context = 10
	interHunkContext = 10
	noprefix = true
",
        "	mnemonicPrefix = true
	external = does-not-exist
	colorMoved = zebra
",
        "	renames = copies
",
        "[color]
	ui = always
",
    ));
    let repository = sandbox.repository("app");
    repository.write(
        "notes.txt",
        "one
two
three
four
",
    );
    repository.commit_all("Initial commit");
    repository.write(
        "notes.txt",
        "ONE
two
three
FOUR
",
    );

    let diff = read_diff(&sandbox.gikli(), &repository, "notes.txt");

    assert_eq!(
        text_of(&diff).differences,
        vec![
            difference(lines(0, 1), lines(0, 1)),
            difference(lines(3, 4), lines(3, 4)),
        ]
    );
    assert_eq!(
        hunks_by_git(&repository, "notes.txt"),
        vec!["-1 +1", "-4 +4"]
    );
}

#[test]
fn a_changed_binary_file_is_shown_without_a_text_diff() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    fs::write(repository.root().join("logo.png"), b"\x89PNG\0one\n").expect("write file");
    repository.commit_all("Initial commit");
    fs::write(repository.root().join("logo.png"), b"\x89PNG\0two\n").expect("write file");

    let diff = read_diff(&sandbox.gikli(), &repository, "logo.png");

    assert_eq!(diff.content, DiffContent::Binary);
    assert_eq!(repository.status_by_git(), vec![" M logo.png"]);
    assert!(binary_by_git(&repository, "logo.png"));
}

#[test]
fn a_new_binary_file_is_shown_without_a_text_diff() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("README.md", "hello\n");
    repository.commit_all("Initial commit");
    fs::write(repository.root().join("logo.png"), b"\x89PNG\0\n").expect("write file");

    let diff = read_diff(&sandbox.gikli(), &repository, "logo.png");

    assert_eq!(diff.content, DiffContent::Binary);
    assert_eq!(repository.status_by_git(), vec!["?? logo.png"]);
}

#[test]
fn a_deleted_binary_file_is_shown_without_a_text_diff() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    fs::write(repository.root().join("logo.png"), b"\x89PNG\0\n").expect("write file");
    repository.commit_all("Initial commit");
    repository.delete("logo.png");

    let diff = read_diff(&sandbox.gikli(), &repository, "logo.png");

    assert_eq!(diff.content, DiffContent::Binary);
    assert!(binary_by_git(&repository, "logo.png"));
}

#[test]
fn a_file_turned_binary_is_shown_without_a_text_diff() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("data.bin", "text for now\n");
    repository.commit_all("Initial commit");
    fs::write(repository.root().join("data.bin"), b"\0\x01\x02").expect("write file");

    let diff = read_diff(&sandbox.gikli(), &repository, "data.bin");

    assert_eq!(diff.content, DiffContent::Binary);
    assert!(binary_by_git(&repository, "data.bin"));
}

/// `.gitattributes` has the last word on what is binary, as it does for git.
#[test]
fn a_file_gitattributes_calls_binary_is_shown_without_a_text_diff() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write(".gitattributes", "*.lock binary\n");
    repository.write("deps.lock", "one\n");
    repository.commit_all("Initial commit");
    repository.write("deps.lock", "two\n");

    let diff = read_diff(&sandbox.gikli(), &repository, "deps.lock");

    assert_eq!(diff.content, DiffContent::Binary);
    assert!(binary_by_git(&repository, "deps.lock"));
}

/// What git compares can be text while the file on disk is not, as with Git LFS, or as here, when
/// `.gitattributes` says to take it as text. The view never shows garbage.
#[test]
fn a_file_that_holds_zero_bytes_is_never_shown_as_text() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write(".gitattributes", "*.dat diff\n");
    repository.write("data.dat", "one\n");
    repository.commit_all("Initial commit");
    fs::write(repository.root().join("data.dat"), b"one\0two\n").expect("write file");

    let diff = read_diff(&sandbox.gikli(), &repository, "data.dat");

    assert_eq!(diff.content, DiffContent::Binary);
    assert!(!binary_by_git(&repository, "data.dat"));
}

#[test]
fn a_nested_repository_is_shown_without_a_text_diff() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("README.md", "hello\n");
    repository.commit_all("Initial commit");
    let nested = sandbox.repository("app/nested");
    nested.write("inner.txt", "inner\n");
    nested.commit_all("Inner commit");

    let diff = read_diff(&sandbox.gikli(), &repository, "nested/");

    assert_eq!(diff.content, DiffContent::Folder);
    assert_eq!(repository.status_by_git(), vec!["?? nested/"]);
}

#[test]
fn a_submodule_is_shown_without_a_text_diff() {
    let sandbox = Sandbox::new();
    sandbox.allow_local_submodules();
    let library = sandbox.repository("library");
    library.write("lib.txt", "one\n");
    library.commit_all("First");
    let repository = sandbox.repository("app");
    repository.git(&[
        "submodule",
        "--quiet",
        "add",
        &file_url(library.root()),
        "vendor/lib",
    ]);
    repository.commit_all("Add the submodule");
    let submodule = sandbox.existing_repository(&repository.root().join("vendor/lib"));
    submodule.write("lib.txt", "two\n");
    submodule.commit_all("Second");

    let diff = read_diff(&sandbox.gikli(), &repository, "vendor/lib");

    assert_eq!(diff.content, DiffContent::Folder);
    assert_eq!(repository.status_by_git(), vec![" M vendor/lib"]);
}

/// A file checked out with CRLF that only differs from HEAD in that way.
#[test]
fn with_autocrlf_a_converted_line_ending_is_no_difference() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.git(&["config", "core.autocrlf", "true"]);
    repository.write("notes.txt", "one\ntwo\n");
    repository.commit_all("Initial commit");
    repository.write("notes.txt", "one\r\ntwo\r\n");

    let diff = read_diff(&sandbox.gikli(), &repository, "notes.txt");

    let text = text_of(&diff);
    assert_eq!(text.head.as_deref(), Some("one\ntwo\n"));
    assert_eq!(text.local.as_deref(), Some("one\ntwo\n"));
    assert_eq!(text.differences, vec![]);
    assert_eq!(repository.git(&["diff", "--stat", "HEAD"]), "");
}

#[test]
fn with_autocrlf_only_the_lines_really_changed_are_differences() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.git(&["config", "core.autocrlf", "true"]);
    repository.write("notes.txt", "one\ntwo\nthree\n");
    repository.commit_all("Initial commit");
    repository.write("notes.txt", "one\r\nTWO\r\nthree\r\n");

    let diff = read_diff(&sandbox.gikli(), &repository, "notes.txt");

    assert_eq!(
        text_of(&diff).differences,
        vec![difference(lines(1, 2), lines(1, 2))]
    );
    assert_eq!(hunks_by_git(&repository, "notes.txt"), vec!["-2 +2"]);
}

#[test]
fn with_autocrlf_input_a_converted_line_ending_is_no_difference() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.git(&["config", "core.autocrlf", "input"]);
    repository.write("notes.txt", "one\ntwo\n");
    repository.commit_all("Initial commit");
    repository.write("notes.txt", "one\r\ntwo\r\n");

    let diff = read_diff(&sandbox.gikli(), &repository, "notes.txt");

    assert_eq!(text_of(&diff).differences, vec![]);
    assert_eq!(repository.git(&["diff", "--stat", "HEAD"]), "");
}

/// Without a conversion, the line ending really is what changed, and git says so. The two sides
/// show the same text, so only the difference tells.
#[test]
fn without_autocrlf_a_changed_line_ending_is_a_difference() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.git(&["config", "core.autocrlf", "false"]);
    repository.write("notes.txt", "one\ntwo\n");
    repository.commit_all("Initial commit");
    repository.write("notes.txt", "one\r\ntwo\n");

    let diff = read_diff(&sandbox.gikli(), &repository, "notes.txt");

    assert_eq!(
        text_of(&diff).differences,
        vec![difference(lines(0, 1), lines(0, 1))]
    );
    assert_eq!(hunks_by_git(&repository, "notes.txt"), vec!["-1 +1"]);
}

/// `count` numbered lines.
fn numbered_lines(count: usize) -> Vec<String> {
    (0..count).map(|line| format!("line {line}")).collect()
}

/// `lines` as the contents of a file.
fn file_of(lines: &[String]) -> String {
    lines.iter().map(|line| format!("{line}\n")).collect()
}

fn region(head: LineRange, local: LineRange) -> UnchangedRegion {
    UnchangedRegion { head, local }
}

#[test]
fn unchanged_lines_are_collapsed_around_a_difference_with_4_lines_of_context() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    let mut contents = numbered_lines(40);
    repository.write("long.txt", &file_of(&contents));
    repository.commit_all("Initial commit");
    contents[20] = "changed".into();
    repository.write("long.txt", &file_of(&contents));

    let diff = read_diff(&sandbox.gikli(), &repository, "long.txt");

    let text = text_of(&diff);
    assert_eq!(
        text.differences,
        vec![difference(lines(20, 21), lines(20, 21))]
    );
    assert_eq!(
        text.collapsed,
        vec![
            region(lines(0, 16), lines(0, 16)),
            region(lines(25, 40), lines(25, 40)),
        ]
    );
    assert_eq!(hunks_by_git(&repository, "long.txt"), vec!["-21 +21"]);
}

#[test]
fn unchanged_lines_between_two_differences_keep_context_on_both_ends() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    let mut contents = numbered_lines(40);
    repository.write("long.txt", &file_of(&contents));
    repository.commit_all("Initial commit");
    contents[10] = "changed".into();
    contents[30] = "changed".into();
    repository.write("long.txt", &file_of(&contents));

    let diff = read_diff(&sandbox.gikli(), &repository, "long.txt");

    assert!(
        text_of(&diff)
            .collapsed
            .contains(&region(lines(15, 26), lines(15, 26)))
    );
    assert_eq!(
        hunks_by_git(&repository, "long.txt"),
        vec!["-11 +11", "-31 +31"]
    );
}

#[test]
fn the_collapsed_lines_of_the_two_sides_stay_aligned_after_lines_were_added() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    let contents = numbered_lines(40);
    repository.write("long.txt", &file_of(&contents));
    repository.commit_all("Initial commit");
    let mut edited = contents.clone();
    edited.splice(
        10..10,
        ["new".to_owned(), "new".to_owned(), "new".to_owned()],
    );
    repository.write("long.txt", &file_of(&edited));

    let diff = read_diff(&sandbox.gikli(), &repository, "long.txt");

    let text = text_of(&diff);
    assert_eq!(
        text.differences,
        vec![difference(lines(10, 10), lines(10, 13))]
    );
    assert_eq!(
        text.collapsed,
        vec![
            region(lines(0, 6), lines(0, 6)),
            region(lines(14, 40), lines(17, 43)),
        ]
    );
    assert_eq!(hunks_by_git(&repository, "long.txt"), vec!["-10,0 +11,3"]);
}

#[test]
fn unchanged_lines_that_fit_in_the_context_are_not_collapsed() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    let mut contents = numbered_lines(13);
    repository.write("short.txt", &file_of(&contents));
    repository.commit_all("Initial commit");
    contents[0] = "changed".into();
    contents[9] = "changed".into();
    repository.write("short.txt", &file_of(&contents));

    let diff = read_diff(&sandbox.gikli(), &repository, "short.txt");

    assert_eq!(text_of(&diff).collapsed, vec![]);
    assert_eq!(
        hunks_by_git(&repository, "short.txt"),
        vec!["-1 +1", "-10 +10"]
    );
}

/// One hidden line would take the place of the marker that says so, so it stays visible.
#[test]
fn a_single_line_past_the_context_is_not_collapsed() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    let mut contents = numbered_lines(6);
    repository.write("short.txt", &file_of(&contents));
    repository.commit_all("Initial commit");
    contents[5] = "changed".into();
    repository.write("short.txt", &file_of(&contents));

    let diff = read_diff(&sandbox.gikli(), &repository, "short.txt");

    assert_eq!(text_of(&diff).collapsed, vec![]);
    assert_eq!(hunks_by_git(&repository, "short.txt"), vec!["-6 +6"]);
}

/// With nothing to review, nothing is hidden.
#[test]
fn a_file_without_differences_is_not_collapsed() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.git(&["config", "core.autocrlf", "true"]);
    let contents = numbered_lines(40);
    repository.write("long.txt", &file_of(&contents));
    repository.commit_all("Initial commit");
    repository.write("long.txt", &file_of(&contents).replace('\n', "\r\n"));

    let diff = read_diff(&sandbox.gikli(), &repository, "long.txt");

    let text = text_of(&diff);
    assert_eq!(text.differences, vec![]);
    assert_eq!(text.collapsed, vec![]);
    assert_eq!(repository.git(&["diff", "--stat", "HEAD"]), "");
}

/// A repository with changes in `b/c.txt`, `a.txt` and `z.txt`, which the changes view shows in
/// that order when it groups them by folder: folders first.
fn repository_with_three_changes(sandbox: &Sandbox) -> TestRepository {
    let repository = sandbox.repository("app");
    for path in ["a.txt", "b/c.txt", "z.txt"] {
        repository.write(path, "one\n");
    }
    repository.commit_all("Initial commit");
    for path in ["a.txt", "b/c.txt", "z.txt"] {
        repository.write(path, "two\n");
    }
    repository
}

#[test]
fn previous_and_next_file_follow_the_changes_view_grouped_by_folder() {
    let sandbox = Sandbox::new();
    let repository = repository_with_three_changes(&sandbox);
    let gikli = sandbox.gikli();

    let first = read_diff(&gikli, &repository, "b/c.txt");
    let middle = read_diff(&gikli, &repository, "a.txt");
    let last = read_diff(&gikli, &repository, "z.txt");

    assert_eq!(first.previous_file, None);
    assert_eq!(first.next_file, Some(PathBuf::from("a.txt")));
    assert_eq!(middle.previous_file, Some(PathBuf::from("b/c.txt")));
    assert_eq!(middle.next_file, Some(PathBuf::from("z.txt")));
    assert_eq!(last.previous_file, Some(PathBuf::from("a.txt")));
    assert_eq!(last.next_file, None);
    assert_eq!(
        repository.status_by_git(),
        vec![" M a.txt", " M b/c.txt", " M z.txt"]
    );
}

#[test]
fn previous_and_next_file_follow_the_changes_view_as_a_flat_list() {
    let sandbox = Sandbox::new();
    let repository = repository_with_three_changes(&sandbox);
    let gikli = sandbox.gikli();
    gikli
        .set_change_grouping(ChangeGrouping::Flat)
        .expect("set grouping");

    let middle = read_diff(&gikli, &repository, "b/c.txt");

    assert_eq!(middle.previous_file, Some(PathBuf::from("a.txt")));
    assert_eq!(middle.next_file, Some(PathBuf::from("z.txt")));
    assert_eq!(
        repository.status_by_git(),
        vec![" M a.txt", " M b/c.txt", " M z.txt"]
    );
}

#[test]
fn after_the_last_difference_the_review_moves_on_to_the_next_file() {
    let sandbox = Sandbox::new();
    let repository = repository_with_three_changes(&sandbox);
    let gikli = sandbox.gikli();

    let diff = read_diff(&gikli, &repository, "a.txt");

    assert!(gikli.settings().go_to_next_file_after_last_change);
    assert_eq!(diff.after_last_difference, Some(PathBuf::from("z.txt")));
    assert_eq!(
        repository.status_by_git(),
        vec![" M a.txt", " M b/c.txt", " M z.txt"]
    );
}

#[test]
fn after_the_last_difference_of_the_last_file_the_review_stays() {
    let sandbox = Sandbox::new();
    let repository = repository_with_three_changes(&sandbox);

    let diff = read_diff(&sandbox.gikli(), &repository, "z.txt");

    assert_eq!(diff.after_last_difference, None);
    assert_eq!(
        repository.status_by_git(),
        vec![" M a.txt", " M b/c.txt", " M z.txt"]
    );
}

#[test]
fn a_setting_keeps_the_review_in_the_file_after_its_last_difference() {
    let sandbox = Sandbox::new();
    let repository = repository_with_three_changes(&sandbox);
    let gikli = sandbox.gikli();
    gikli
        .set_go_to_next_file_after_last_change(false)
        .expect("change the setting");

    let diff = read_diff(&gikli, &repository, "a.txt");

    assert_eq!(diff.after_last_difference, None);
    assert_eq!(diff.next_file, Some(PathBuf::from("z.txt")));
    assert!(
        !sandbox.gikli().settings().go_to_next_file_after_last_change,
        "the setting survives a restart"
    );
    assert_eq!(
        repository.status_by_git(),
        vec![" M a.txt", " M b/c.txt", " M z.txt"]
    );
}

/// The Changelists come first, in their order, then Unversioned Files.
#[test]
fn previous_and_next_file_go_through_the_changelists_and_then_unversioned_files() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    for path in ["a.txt", "z.txt"] {
        repository.write(path, "one\n");
    }
    repository.commit_all("Initial commit");
    let git_dir = repository.git(&["rev-parse", "--absolute-git-dir"]);
    let store = PathBuf::from(git_dir.trim()).join("gikli");
    std::fs::create_dir_all(&store).expect("create folder");
    std::fs::write(
        store.join("changelists.json"),
        r#"{
          "version": 1,
          "active": "feature",
          "head": null,
          "changelists": [
            { "id": "changes", "name": "Changes", "files": ["z.txt"] },
            { "id": "feature", "name": "Feature", "files": [] }
          ]
        }"#,
    )
    .expect("write Changelists");
    repository.write("z.txt", "two\n");
    repository.write("a.txt", "two\n");
    repository.write("new.txt", "new\n");
    let gikli = sandbox.gikli();

    let first = read_diff(&gikli, &repository, "z.txt");
    let middle = read_diff(&gikli, &repository, "a.txt");
    let last = read_diff(&gikli, &repository, "new.txt");

    assert_eq!(first.previous_file, None);
    assert_eq!(first.next_file, Some(PathBuf::from("a.txt")));
    assert_eq!(middle.previous_file, Some(PathBuf::from("z.txt")));
    assert_eq!(middle.next_file, Some(PathBuf::from("new.txt")));
    assert_eq!(last.previous_file, Some(PathBuf::from("a.txt")));
    assert_eq!(last.next_file, None);
    assert_eq!(
        repository.status_by_git(),
        vec![" M a.txt", " M z.txt", "?? new.txt"]
    );
}

/// An ignored file is listed, but it is no change to review.
#[test]
fn previous_and_next_file_pass_over_ignored_files() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write(".gitignore", "m.txt\n");
    repository.write("a.txt", "one\n");
    repository.write("z.txt", "one\n");
    repository.commit_all("Initial commit");
    repository.write("a.txt", "two\n");
    repository.write("m.txt", "ignored\n");
    repository.write("z.txt", "two\n");
    let gikli = sandbox.gikli();
    gikli.set_show_ignored_files(true).expect("show ignored");

    let diff = read_diff(&gikli, &repository, "a.txt");

    assert_eq!(diff.next_file, Some(PathBuf::from("z.txt")));
    assert_eq!(repository.status_by_git(), vec![" M a.txt", " M z.txt"]);
}

/// A file the user just took back to what HEAD has stops being a change, while they still look at
/// it, and the review goes on from where it was.
#[test]
fn a_file_that_is_no_longer_changed_keeps_its_place_between_the_others() {
    let sandbox = Sandbox::new();
    let repository = repository_with_three_changes(&sandbox);
    repository.write("a.txt", "one\n");

    let diff = read_diff(&sandbox.gikli(), &repository, "a.txt");

    assert_eq!(text_of(&diff).differences, vec![]);
    assert_eq!(diff.previous_file, Some(PathBuf::from("b/c.txt")));
    assert_eq!(diff.next_file, Some(PathBuf::from("z.txt")));
    assert_eq!(repository.status_by_git(), vec![" M b/c.txt", " M z.txt"]);
}

/// Saves `text` as the local side of the file at `path`, as last read.
fn save(
    gikli: &Gikli,
    repository: &TestRepository,
    path: &str,
    text: &str,
) -> Result<FileDiff, Failure> {
    let opened = opened(gikli, repository);
    let diff = gikli.read_diff(&opened, path).expect("read diff");
    gikli.save_local_file(&opened, path, text, &text_of(&diff).local_version)
}

/// The bytes on disk of the file at `path`.
fn bytes_on_disk(repository: &TestRepository, path: &str) -> Vec<u8> {
    fs::read(repository.root().join(path)).expect("read file")
}

#[test]
fn the_local_side_can_be_edited_and_saved() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("notes.txt", "one\ntwo\n");
    repository.commit_all("Initial commit");
    repository.write("notes.txt", "one\nTWO\n");
    let gikli = sandbox.gikli();

    assert_eq!(
        text_of(&read_diff(&gikli, &repository, "notes.txt")).local_read_only,
        None
    );
    let saved = save(&gikli, &repository, "notes.txt", "ONE\nTWO\nthree\n").expect("save");

    let text = text_of(&saved);
    assert_eq!(text.local.as_deref(), Some("ONE\nTWO\nthree\n"));
    assert_eq!(text.differences, vec![difference(lines(0, 2), lines(0, 3))]);
    assert_eq!(
        bytes_on_disk(&repository, "notes.txt"),
        b"ONE\nTWO\nthree\n"
    );
    assert_eq!(repository.status_by_git(), vec![" M notes.txt"]);
}

#[test]
fn saving_what_head_has_leaves_no_change() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("notes.txt", "one\n");
    repository.commit_all("Initial commit");
    repository.write("notes.txt", "two\n");

    let saved = save(&sandbox.gikli(), &repository, "notes.txt", "one\n").expect("save");

    assert_eq!(text_of(&saved).differences, vec![]);
    assert_eq!(repository.status_by_git(), Vec::<String>::new());
}

#[test]
fn a_new_file_can_be_edited_and_saved() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("new.txt", "one\n");

    let saved = save(&sandbox.gikli(), &repository, "new.txt", "one\ntwo\n").expect("save");

    assert_eq!(
        text_of(&saved).differences,
        vec![difference(lines(0, 0), lines(0, 2))]
    );
    assert_eq!(bytes_on_disk(&repository, "new.txt"), b"one\ntwo\n");
    assert_eq!(repository.status_by_git(), vec!["?? new.txt"]);
}

#[test]
fn a_setting_turns_editing_the_local_side_off() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("notes.txt", "one\n");
    repository.commit_all("Initial commit");
    repository.write("notes.txt", "two\n");
    let gikli = sandbox.gikli();
    assert!(gikli.settings().diff_local_side_editable);
    let opened = opened(&gikli, &repository);
    let before = gikli.read_diff(&opened, "notes.txt").expect("read diff");
    gikli
        .set_diff_local_side_editable(false)
        .expect("change the setting");

    let after = read_diff(&gikli, &repository, "notes.txt");
    let attempt = gikli.save_local_file(
        &opened,
        "notes.txt",
        "three\n",
        &text_of(&before).local_version,
    );

    assert_eq!(
        text_of(&after).local_read_only,
        Some(LocalReadOnly::TurnedOff)
    );
    assert_eq!(
        attempt.unwrap_err(),
        Failure::LocalSideNotEditable {
            path: PathBuf::from("notes.txt"),
        }
    );
    assert_eq!(bytes_on_disk(&repository, "notes.txt"), b"two\n");
    assert!(
        !sandbox.gikli().settings().diff_local_side_editable,
        "the setting survives a restart"
    );
    assert_eq!(repository.status_by_git(), vec![" M notes.txt"]);
}

/// Whoever saved last would otherwise silently undo the other's work.
#[test]
fn a_file_changed_on_disk_after_it_was_read_is_not_overwritten() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("notes.txt", "one\n");
    repository.commit_all("Initial commit");
    repository.write("notes.txt", "two\n");
    let gikli = sandbox.gikli();
    let opened = opened(&gikli, &repository);
    let read = gikli.read_diff(&opened, "notes.txt").expect("read diff");
    repository.write("notes.txt", "from another editor\n");

    let attempt = gikli.save_local_file(
        &opened,
        "notes.txt",
        "mine\n",
        &text_of(&read).local_version,
    );

    assert_eq!(
        attempt.unwrap_err(),
        Failure::LocalFileChanged {
            path: PathBuf::from("notes.txt"),
        }
    );
    assert_eq!(
        bytes_on_disk(&repository, "notes.txt"),
        b"from another editor\n"
    );
    assert_eq!(repository.status_by_git(), vec![" M notes.txt"]);
}

#[test]
fn a_file_deleted_after_it_was_read_is_not_brought_back() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("notes.txt", "one\n");
    repository.commit_all("Initial commit");
    repository.write("notes.txt", "two\n");
    let gikli = sandbox.gikli();
    let opened = opened(&gikli, &repository);
    let read = gikli.read_diff(&opened, "notes.txt").expect("read diff");
    repository.delete("notes.txt");

    let attempt = gikli.save_local_file(
        &opened,
        "notes.txt",
        "mine\n",
        &text_of(&read).local_version,
    );

    assert_eq!(
        attempt.unwrap_err(),
        Failure::LocalFileChanged {
            path: PathBuf::from("notes.txt"),
        }
    );
    assert!(!repository.root().join("notes.txt").exists());
    assert_eq!(repository.status_by_git(), vec![" D notes.txt"]);
}

#[test]
fn a_deleted_file_has_no_local_side_to_edit() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("gone.txt", "one\n");
    repository.commit_all("Initial commit");
    repository.delete("gone.txt");
    let gikli = sandbox.gikli();

    let diff = read_diff(&gikli, &repository, "gone.txt");
    let attempt = save(&gikli, &repository, "gone.txt", "back\n");

    assert_eq!(
        text_of(&diff).local_read_only,
        Some(LocalReadOnly::NoLocalFile)
    );
    assert_eq!(
        attempt.unwrap_err(),
        Failure::LocalSideNotEditable {
            path: PathBuf::from("gone.txt"),
        }
    );
    assert!(!repository.root().join("gone.txt").exists());
    assert_eq!(repository.status_by_git(), vec![" D gone.txt"]);
}

/// The view would show the bytes it cannot read as replacement characters, and saving would write
/// those characters over them.
#[test]
fn a_file_that_is_not_utf8_can_be_read_but_not_edited() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("latin1.txt", "one\n");
    repository.commit_all("Initial commit");
    fs::write(repository.root().join("latin1.txt"), b"a\xf1o\n").expect("write file");
    let gikli = sandbox.gikli();

    let diff = read_diff(&gikli, &repository, "latin1.txt");
    let attempt = save(&gikli, &repository, "latin1.txt", "mine\n");

    let text = text_of(&diff);
    assert_eq!(text.local.as_deref(), Some("a\u{fffd}o\n"));
    assert_eq!(text.local_read_only, Some(LocalReadOnly::NotUtf8));
    assert_eq!(
        attempt.unwrap_err(),
        Failure::LocalSideNotEditable {
            path: PathBuf::from("latin1.txt"),
        }
    );
    assert_eq!(bytes_on_disk(&repository, "latin1.txt"), b"a\xf1o\n");
    assert_eq!(repository.status_by_git(), vec![" M latin1.txt"]);
}

#[test]
fn a_path_outside_the_repository_is_not_written() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("notes.txt", "one\n");
    repository.commit_all("Initial commit");
    let outside = sandbox.root().join("outside.txt");
    fs::write(&outside, "keep\n").expect("write file");
    let gikli = sandbox.gikli();
    let opened = opened(&gikli, &repository);

    let attempt = gikli.save_local_file(&opened, "../outside.txt", "mine\n", "any");

    assert_eq!(
        attempt.unwrap_err(),
        Failure::LocalSideNotEditable {
            path: PathBuf::from("../outside.txt"),
        }
    );
    assert_eq!(fs::read(outside).expect("read file"), b"keep\n");
    assert_eq!(repository.status_by_git(), Vec::<String>::new());
}

#[test]
fn saving_a_file_with_crlf_keeps_crlf_on_every_line() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.git(&["config", "core.autocrlf", "true"]);
    repository.write("notes.txt", "one\ntwo\nthree\n");
    repository.commit_all("Initial commit");
    repository.write("notes.txt", "one\r\ntwo\r\nthree\r\n");

    let saved = save(
        &sandbox.gikli(),
        &repository,
        "notes.txt",
        "one\nTWO\nnew\nthree\n",
    )
    .expect("save");

    assert_eq!(
        bytes_on_disk(&repository, "notes.txt"),
        b"one\r\nTWO\r\nnew\r\nthree\r\n"
    );
    assert_eq!(
        text_of(&saved).differences,
        vec![difference(lines(1, 2), lines(1, 3))]
    );
    assert_eq!(hunks_by_git(&repository, "notes.txt"), vec!["-2 +2,2"]);
}

/// Without a conversion, a different line ending on a line would be a change to that line.
#[test]
fn saving_keeps_the_line_ending_of_each_line_of_a_file_that_mixes_them() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.git(&["config", "core.autocrlf", "false"]);
    let mixed = "one\r\ntwo\nthree\r\nfour\nfive\r\n";
    repository.write("notes.txt", mixed);
    repository.commit_all("Initial commit");

    let saved = save(
        &sandbox.gikli(),
        &repository,
        "notes.txt",
        "one\ntwo\nTHREE\nfour\nfive\n",
    )
    .expect("save");

    assert_eq!(
        bytes_on_disk(&repository, "notes.txt"),
        b"one\r\ntwo\nTHREE\r\nfour\nfive\r\n"
    );
    assert_eq!(
        text_of(&saved).differences,
        vec![difference(lines(2, 3), lines(2, 3))]
    );
    assert_eq!(hunks_by_git(&repository, "notes.txt"), vec!["-3 +3"]);
}

#[test]
fn saving_unchanged_text_leaves_the_file_as_it_was() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.git(&["config", "core.autocrlf", "false"]);
    let mixed = "one\r\ntwo\nthree\r\nno ending";
    repository.write("notes.txt", mixed);
    let gikli = sandbox.gikli();
    let diff = read_diff(&gikli, &repository, "notes.txt");
    let shown = text_of(&diff).local.clone().expect("a local side");

    save(&gikli, &repository, "notes.txt", &shown).expect("save");

    assert_eq!(bytes_on_disk(&repository, "notes.txt"), mixed.as_bytes());
    assert_eq!(repository.status_by_git(), vec!["?? notes.txt"]);
}

#[test]
fn a_last_line_without_a_line_ending_stays_without_one() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.git(&["config", "core.autocrlf", "false"]);
    repository.write("notes.txt", "one\r\ntwo");

    save(&sandbox.gikli(), &repository, "notes.txt", "one\nTWO").expect("save");

    assert_eq!(bytes_on_disk(&repository, "notes.txt"), b"one\r\nTWO");
    assert_eq!(repository.status_by_git(), vec!["?? notes.txt"]);
}

/// A carriage return in the middle of a line is part of the line to git, so it stays where it is.
#[test]
fn a_carriage_return_inside_a_line_stays_in_the_line() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.git(&["config", "core.autocrlf", "false"]);
    repository.write("notes.txt", "one\rstill one\ntwo\n");
    repository.commit_all("Initial commit");
    repository.write("notes.txt", "one\rstill one\nTWO\n");

    let diff = read_diff(&sandbox.gikli(), &repository, "notes.txt");

    let text = text_of(&diff);
    assert_eq!(text.local.as_deref(), Some("one\rstill one\nTWO\n"));
    assert_eq!(text.differences, vec![difference(lines(1, 2), lines(1, 2))]);
    assert_eq!(hunks_by_git(&repository, "notes.txt"), vec!["-2 +2"]);
}

/// git compares where a link points, not what it points at, so that is what the view shows.
#[cfg(unix)]
#[test]
fn a_symbolic_link_shows_where_it_points_and_cannot_be_edited() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("target.txt", "contents\n");
    std::os::unix::fs::symlink("target.txt", repository.root().join("link")).expect("link");
    repository.commit_all("Initial commit");
    repository.delete("link");
    std::os::unix::fs::symlink("elsewhere.txt", repository.root().join("link")).expect("link");
    let gikli = sandbox.gikli();

    let diff = read_diff(&gikli, &repository, "link");
    let attempt = save(&gikli, &repository, "link", "mine\n");

    let text = text_of(&diff);
    assert_eq!(text.head.as_deref(), Some("target.txt"));
    assert_eq!(text.local.as_deref(), Some("elsewhere.txt"));
    assert_eq!(text.local_read_only, Some(LocalReadOnly::SymbolicLink));
    assert_eq!(
        attempt.unwrap_err(),
        Failure::LocalSideNotEditable {
            path: PathBuf::from("link"),
        }
    );
    assert_eq!(repository.status_by_git(), vec![" M link"]);
}

/// git writes this as the deletion of a link and the addition of a file, both from the top. On a
/// machine that cannot make links, the index still says what HEAD has.
#[test]
fn a_link_that_became_a_file_is_one_difference() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.git(&["config", "core.symlinks", "true"]);
    repository.write("where.txt", "elsewhere.txt");
    let blob = repository.git(&["hash-object", "-w", "where.txt"]);
    let entry = format!("120000,{},link", blob.trim());
    repository.git(&["update-index", "--add", "--cacheinfo", &entry]);
    repository.git(&["commit", "--quiet", "--message", "Add a link"]);
    repository.write("link", "one\ntwo\nthree\n");

    let diff = read_diff(&sandbox.gikli(), &repository, "link");

    let text = text_of(&diff);
    assert_eq!(text.head.as_deref(), Some("elsewhere.txt"));
    assert_eq!(text.local.as_deref(), Some("one\ntwo\nthree\n"));
    assert_eq!(text.differences, vec![difference(lines(0, 1), lines(0, 3))]);
    assert_eq!(repository.git(&["show", "HEAD:link"]), "elsewhere.txt");
}
