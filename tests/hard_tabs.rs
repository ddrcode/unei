//! Makefiles are written with hard tabs: a recipe line must start with one,
//! so there `Tab` inserts a tab and the shifts move by one — whatever
//! `expandtab` says for every other file. The rule is the language's
//! (`LangSpec::hard_tabs`), not an option. IJKL: `k` is down, `h` inserts.

use unei::editor::Editor;
use unei::editor::testing::{editor_with_buffers, feed, text};

fn open(name: &str, content: &str) -> Editor {
    editor_with_buffers(&[(name, content)])
}

#[test]
fn tab_inserts_a_tab_in_a_makefile_and_spaces_elsewhere() {
    let mut ed = open("Makefile", "all: main.o\n\n");
    feed(&mut ed, "kh<Tab>gcc -o app main.o<Esc>");
    assert_eq!(text(&ed), "all: main.o\n\tgcc -o app main.o\n");

    let mut ed = open("main.rs", "fn main() {\n\n}\n");
    feed(&mut ed, "kh<Tab>x<Esc>");
    assert_eq!(text(&ed), "fn main() {\n    x\n}\n");
}

#[test]
fn shifts_move_by_a_tab_in_a_makefile() {
    let mut ed = open("platforms/virt.mk", "gcc\n");
    feed(&mut ed, ">>");
    assert_eq!(text(&ed), "\tgcc\n");
    feed(&mut ed, ">>");
    assert_eq!(text(&ed), "\t\tgcc\n");
    feed(&mut ed, "<lt><lt>");
    assert_eq!(text(&ed), "\tgcc\n");
    // insert-mode Ctrl+T / Ctrl+D follow the same rule
    feed(&mut ed, "A<C-t>x<Esc>");
    assert_eq!(text(&ed), "\t\tgccx\n");
    feed(&mut ed, "A<C-d>y<Esc>");
    assert_eq!(text(&ed), "\tgccxy\n");

    let mut ed = open("main.rs", "gcc\n");
    feed(&mut ed, ">>");
    assert_eq!(text(&ed), "    gcc\n", "spaces, as before");
}

#[test]
fn enter_carries_the_recipe_tab_to_the_next_line() {
    // following the current indentation, as everywhere: no tab is computed
    // after a rule header, the one typed on the recipe line is kept
    let mut ed = open("Makefile", "all:\n\tgcc\n");
    feed(&mut ed, "kA<CR>ld<Esc>");
    assert_eq!(text(&ed), "all:\n\tgcc\n\tld\n");
}
