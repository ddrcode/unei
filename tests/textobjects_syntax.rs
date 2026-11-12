//! Syntax-aware text objects (#78): `af`/`nf` (function) and `ac`/`nc` (class)
//! resolved via tree-sitter. Layout reminder: i=up, k=down, j=left, l=right.

use unei::editor::testing::{editor_with_buffers, feed, text};

fn run(src: &str, keys: &str) -> String {
    let mut e = editor_with_buffers(&[("t.rs", src)]);
    feed(&mut e, keys);
    text(&e)
}

#[test]
fn daf_deletes_the_whole_function() {
    assert_eq!(
        run("fn a() {\n    x();\n}\nfn b() {}\n", "daf"),
        "fn b() {}\n"
    );
}

#[test]
fn daf_targets_the_function_the_cursor_is_in() {
    // down to fn b, delete it, leave fn a
    assert_eq!(
        run("fn a() {}\nfn b() {\n    y();\n}\n", "kkkdaf"),
        "fn a() {}\n"
    );
}

#[test]
fn nf_is_the_body_keeping_the_signature() {
    assert_eq!(
        run("fn a() {\n    x();\n    y();\n}\n", "kdnf"),
        "fn a() {\n}\n"
    );
}

#[test]
fn nf_takes_the_line_of_a_one_statement_body() {
    // vim's `di{`: a body on lines of its own is linewise, even one line
    assert_eq!(run("fn a() {\n    x();\n}\n", "kdnf"), "fn a() {\n}\n");
    // a body sharing the brace's line stays charwise
    assert_eq!(run("fn a() { x(); }\n", "lllllllldnf"), "fn a() {  }\n");
}

#[test]
fn dac_deletes_the_enclosing_impl_from_a_method() {
    let src = "impl Foo {\n    fn m(&self) {\n        z();\n    }\n}\nfn keep() {}\n";
    assert_eq!(run(src, "kkdac"), "fn keep() {}\n");
}

#[test]
fn daf_inside_an_impl_removes_only_the_method() {
    let src = "impl Foo {\n    fn m(&self) {\n        z();\n    }\n}\n";
    assert_eq!(run(src, "kkdaf"), "impl Foo {\n}\n");
}

#[test]
fn caf_replaces_a_function() {
    // change-around-function opens an insert where the function was
    assert_eq!(
        run("fn old() {\n    a();\n}\n", "caffn new() {}<Esc>"),
        "fn new() {}\n"
    );
}

// --- SystemVerilog (#117): bodies are delimited, not braced ---------------

fn run_sv(src: &str, keys: &str) -> String {
    let mut e = editor_with_buffers(&[("t.sv", src)]);
    feed(&mut e, keys);
    text(&e)
}

const SV_MOD: &str = "module m;\n  function int f(int a);\n    f = a;\n    return f;\n  endfunction\n  logic x;\nendmodule\nmodule keep;\nendmodule\n";

#[test]
fn sv_daf_deletes_the_routine_from_inside_it() {
    // down twice: on `f = a;`
    assert_eq!(
        run_sv(SV_MOD, "kkdaf"),
        "module m;\n  logic x;\nendmodule\nmodule keep;\nendmodule\n"
    );
}

#[test]
fn sv_nf_is_the_statements_between_the_header_and_endfunction() {
    assert_eq!(
        run_sv(SV_MOD, "kkdnf"),
        "module m;\n  function int f(int a);\n  endfunction\n  logic x;\nendmodule\nmodule keep;\nendmodule\n"
    );
}

#[test]
fn sv_dac_deletes_the_enclosing_module() {
    assert_eq!(run_sv(SV_MOD, "kkdac"), "module keep;\nendmodule\n");
}

#[test]
fn sv_nc_is_the_items_between_the_header_and_endmodule() {
    assert_eq!(
        run_sv(SV_MOD, "kkdnc"),
        "module m;\nendmodule\nmodule keep;\nendmodule\n"
    );
}

#[test]
fn sv_class_inner_spans_past_a_nested_methods_own_delimiters() {
    // the method's `;` and `endfunction` lie inside the class: the class's
    // inner is still bounded by its own `;` and `endclass`
    let src = "class C;\n  int x;\n  function void m();\n    x = 1;\n  endfunction\nendclass\n";
    assert_eq!(run_sv(src, "kkkdnc"), "class C;\nendclass\n");
    assert_eq!(
        run_sv(src, "kkkdnf"),
        "class C;\n  int x;\n  function void m();\n  endfunction\nendclass\n"
    );
}

#[test]
fn sv_package_inner() {
    let src = "package p;\n  localparam int N = 1;\n  typedef int t;\nendpackage\n";
    assert_eq!(run_sv(src, "kdnc"), "package p;\nendpackage\n");
}
