from thoxa.notebook import NotebookSession


def test_bindings_persist_across_cells():
    s = NotebookSession()
    s.run_cell("let x = 10")
    r = s.run_cell('println("{x + 5}")')
    assert r["output"] == "15\n"
    assert "error" not in r


def test_last_expression_value_is_shown():
    s = NotebookSession()
    assert s.run_cell("1 + 2 * 3")["value"] == "7"


def test_definitions_persist_across_cells():
    s = NotebookSession()
    s.run_cell("fn double(n!: Int64) -> Int64 { n * 2 }")
    assert s.run_cell("double(21)")["value"] == "42"


def test_type_and_method_persist():
    s = NotebookSession()
    s.run_cell("type Box(n: Int64) { export fn get() -> Int64 { n } }")
    assert s.run_cell("Box(n: 99).get()")["value"] == "99"


def test_error_is_reported_not_raised():
    s = NotebookSession()
    r = s.run_cell("let y = nope")
    assert "error" in r and "not defined" in r["error"]


def test_void_result_shows_no_value():
    s = NotebookSession()
    r = s.run_cell("let z = 1")
    assert r.get("value") is None
