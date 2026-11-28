import importlib.machinery
import importlib.util
import unittest
from pathlib import Path

path = Path(__file__).resolve().parents[1] / "depgraph"
spec = importlib.util.spec_from_loader("depgraph", importlib.machinery.SourceFileLoader("depgraph", str(path)))
depgraph = importlib.util.module_from_spec(spec)
spec.loader.exec_module(depgraph)


class WithoutCfgTest(unittest.TestCase):
    def test_hides_only_the_test_item(self):
        text = (
            '#[cfg(test)]\n#[path = "t.rs"]\nmod tests;\n'
            'use a::b;\n'
            '#[cfg(test)]\nuse x::{y, z};\n'
            '#[cfg(all(test, unix))]\nfn t() { let s = "}"; /* } */ let c = \'}\'; }\n'
            '#[cfg(test)]\nmod tests {\n    fn a() { sleep(1); }\n}\n'
            'fn real() { sleep(2); }\n'
        )
        out = depgraph.without_cfg_test(text)
        self.assertIn("use a::b;", out)
        self.assertIn("fn real() { sleep(2); }", out)
        self.assertNotIn("sleep(1)", out)
        self.assertNotIn("x::", out)
        self.assertNotIn("fn t", out)
        self.assertEqual(out.count("\n"), text.count("\n"))

    def test_keeps_non_test_cfg(self):
        text = '#[cfg(not(test))]\nfn a() {}\n#[cfg(unix)]\nfn b() {}\n'
        self.assertEqual(depgraph.without_cfg_test(text), text)

    def blanked(self, text):
        out = depgraph.without_cfg_test(text)
        self.assertEqual(out.count("\n"), text.count("\n"))
        return out

    def test_field_variant_and_arm_end_at_their_comma(self):
        out = self.blanked("struct S { #[cfg(test)] a: u8, b: X }\nimpl Y {}\n")
        self.assertIn("b: X }", out)
        self.assertIn("impl Y {}", out)
        self.assertNotIn("a: u8", out)
        out = self.blanked("enum E { #[cfg(test)] A(u8), B }\nfn z() {}\n")
        self.assertIn("B }", out)
        self.assertNotIn("A(", out)
        out = self.blanked("match x {\n #[cfg(test)] 1 => foo(),\n _ => bar(),\n}\n")
        self.assertIn("_ => bar()", out)
        self.assertNotIn("foo", out)

    def test_last_field_without_comma_stops_at_the_closer(self):
        out = self.blanked("struct S { b: X, #[cfg(test)] a: u8 }\nimpl Y {}\n")
        self.assertIn("b: X,", out)
        self.assertIn("}\nimpl Y {}", out)
        self.assertNotIn("a: u8", out)

    def test_only_a_positive_test_requirement_blanks(self):
        for pred in ("not(test)", "all(not(test), unix)", 'all(feature = "test-utils", unix)', "any(test, unix)"):
            text = f"#[cfg({pred})]\nfn a() {{}}\n"
            self.assertEqual(depgraph.without_cfg_test(text), text, pred)

    def test_const_fn_ends_at_its_body(self):
        out = self.blanked("#[cfg(test)]\nconst fn a() -> u8 { 1 }\nfn keep() {}\n")
        self.assertIn("fn keep() {}", out)
        self.assertNotIn("const fn", out)

    def test_markers_in_comments_and_strings_do_not_trigger(self):
        text = '// #[cfg(test)]\nfn a() { let s = "#[cfg(test)]"; }\n/* #[cfg(test)] */\nfn b() {}\n'
        self.assertEqual(depgraph.without_cfg_test(text), text)


RUST = {
    "src/a.rs": '''
pub struct Hub { tx: tokio::sync::watch::Sender<u8>, low: Arc<Low> }
impl Hub { pub fn publish(&self, v: u8) { let _ = self.tx.send(v); } }

pub struct Top { low: Arc<Low>, hub: Hub, weak: Weak<Side>, plain: Vec<u8> }
impl Top {
    pub fn go(&self) {
        self.low.read();
        self.low.stop();
        self.hub.publish(1);
        // self.weak.ignored();
    }
}

pub struct Low { items: Mutex<BTreeMap<SessionId, Row>> }
impl Low {
    pub fn read(&self) -> usize { 0 }
    pub fn stop(&self) -> Result<()> { Ok(()) }
    pub fn add(&self, r: Row) { self.items.lock().insert(r.id, r); }
    pub fn tell(&self, hub: &Hub) { hub.publish(2); }
}

pub struct Side { low: Arc<Low>, top: Arc<Top> }
impl Side { fn hi(&self) { self.top.go(); } }

pub struct Borrowed<'a> { top: &'a Arc<Top> }

#[cfg(test)]
pub struct OnlyInTests { x: Arc<Top> }

pub async fn serve(top: &Arc<Top>) { top.go(); top.low.read(); }
pub fn unrelated(n: u8) { n.go(); }

pub struct Copy1 { items: Mutex<BTreeMap<SessionId, Row>>, tx: Sender<u8> }
impl Copy1 { fn put(&self, r: Row) { self.items.lock().insert(r.id, r); } }
''',
}


class RustStructs(unittest.TestCase):
    def setUp(self):
        self.info = depgraph.build_rust(RUST)
        self.by_name = {n["name"]: i for i, n in self.info["nodes"].items()}
        self.kinds = {(self.info["nodes"][e["from"]]["name"], self.info["nodes"][e["to"]]["name"], e["kind"]): e for e in self.info["edges"]}

    def test_nodes_are_long_lived_structs_without_tests_or_borrowers(self):
        names = set(self.by_name)
        self.assertTrue({"Hub", "Top", "Low", "Side", "Copy1", "fns in a"} <= names)
        self.assertFalse({"Borrowed", "OnlyInTests", "fns in unrelated"} & names)

    def test_field_edges_and_weak(self):
        self.assertIn(("Top", "Low", "owns"), self.kinds)
        self.assertIn(("Top", "Hub", "owns"), self.kinds)
        self.assertTrue(self.kinds[("Top", "Side", "owns")]["weak"])
        self.assertFalse(self.kinds[("Top", "Low", "owns")]["weak"])

    def test_call_edges_split_ask_order_push_and_ignore_comments(self):
        self.assertIn(("Top", "Low", "ask"), self.kinds)
        self.assertIn(("Top", "Low", "order"), self.kinds)
        self.assertIn(("Top", "Hub", "push"), self.kinds)
        self.assertNotIn(("Top", "Side", "ask"), self.kinds)
        self.assertIn("go", self.kinds[("Side", "Top", "order")]["label"])

    def test_free_fn_serves_param_and_its_fields(self):
        self.assertIn(("fns in a", "Top", "order"), self.kinds)
        self.assertIn(("fns in a", "Low", "ask"), self.kinds)

    def test_kinds(self):
        n = self.info["nodes"]
        self.assertEqual(n[self.by_name["Low"]]["kind"], "lock")
        self.assertEqual(n[self.by_name["Hub"]]["kind"], "store")

    def test_cycle_group_cut_and_upward_push(self):
        result = depgraph.analyse(self.info["nodes"], self.info["edges"], self.info)
        self.assertEqual(len(result["cycle_groups"]), 1)
        group = result["cycle_groups"][0]
        self.assertEqual({self.info["nodes"][m]["name"] for m in group["members"]}, {"Top", "Side"})
        self.assertEqual(len(group["cuts"]), 1)
        self.assertEqual([(self.info["nodes"][e["from"]]["name"], self.info["nodes"][e["to"]]["name"]) for e in result["upward"]], [("Low", "Hub")])

    def test_push_that_closes_a_cycle_is_upward_not_a_cycle(self):
        src = {"x.rs": "pub struct A { b: Arc<B> }\nimpl A { fn go(&self) { self.b.run(); } }\n"
                       "pub struct B { a: Arc<Hub> }\nimpl B { fn run(&self) { self.a.publish(); } fn on(&self) {} }\n"
                       "pub struct Hub { t: Sender<u8> }\nimpl Hub { fn publish(&self) {} }\n"}
        info = depgraph.build_rust(src)
        result = depgraph.analyse(info["nodes"], info["edges"], info)
        self.assertEqual(result["cycle_groups"], [])
        info["edges"].append(dict(**{"from": "x.rs::B", "to": "x.rs::A"}, kind="push", label="", weak=False, count=1))
        result = depgraph.analyse(info["nodes"], info["edges"], info)
        self.assertEqual(result["cycle_groups"], [])
        self.assertEqual(len(result["upward"]), 1)

    def test_duplicated_state_lists_writers(self):
        rows = depgraph.duplicated_state(self.info)
        self.assertEqual(len(rows), 1)
        norm, name, who = rows[0]
        self.assertEqual(name, "items")
        self.assertEqual({w[0] for w in who}, {"Low", "Copy1"})
        self.assertEqual({tuple(w[2]) for w in who}, {("add",), ("put",)})


WRITERS = {
    "w.rs": '''
pub struct S { map: Mutex<HashMap<SessionId, Item>>, tracked: Mutex<HashMap<SessionId, Item>>, other: Arc<S2> }
pub struct S2(Mutex<Vec<Item>>);
impl S {
    fn add(&self) { crate::lock(&self.map).insert(1, Item); }
    fn track(&self) { let mut g = self.tracked.lock(); g.insert(1, Item); }
    fn peek(&self) -> usize { self.tracked.lock().len() }
}
impl S2 {
    fn zero(&self) { self.0.lock().push(Item); }
    fn read(&self) -> usize { self.0.lock().len() }
}
''',
}


class WriterDetection(unittest.TestCase):
    def test_lock_helper_guard_local_and_tuple_field(self):
        info = depgraph.build_rust(WRITERS)
        w = {(k[0].split("::")[-1], k[1]): v for k, v in info["writers"].items()}
        self.assertEqual(w[("S", "map")], ["add"])
        self.assertEqual(w[("S", "tracked")], ["track"])
        self.assertEqual(w[("S2", "_0")], ["zero"])


REACH = {
    "r.rs": '''
type Shared = Arc<Low>;
pub struct Low { m: Mutex<u8> }
impl Low { pub fn read(&self) -> u8 { 0 } pub fn stop(&self) {} }
pub struct A { low: Shared, locked: Mutex<Low>, maybe: Option<Arc<Low>> }
impl A {
    fn split(&self) {
        self
            .low
            .read();
    }
    fn guard(&self) { self.locked.lock().read(); }
    fn guard_local(&self) { let g = self.locked.lock(); g.stop(); }
    fn helper(&self) { crate::lock(&self.locked).stop(); }
    fn some(&self) { if let Some(b) = &self.maybe { b.read(); } }
}
''',
}


class ReachShapes(unittest.TestCase):
    def test_edges_through_splits_guards_options_and_aliases(self):
        info = depgraph.build_rust(REACH)
        got = {(info["nodes"][e["from"]]["name"], info["nodes"][e["to"]]["name"], e["kind"]): e for e in info["edges"]}
        self.assertIn(("A", "Low", "owns"), got)
        self.assertIn(("A", "Low", "ask"), got)
        self.assertIn(("A", "Low", "order"), got)
        self.assertEqual(got[("A", "Low", "ask")]["count"], 3)
        self.assertEqual(got[("A", "Low", "order")]["count"], 2)


CLASH = {
    "a.rs": "struct Turn { n: u8 }\nstruct Item { n: u8 }\npub struct X { turn: Option<Turn>, item: Item, tx: Sender<u8> }\n",
    "b.rs": "struct Turn { m: u16 }\nstruct Item { n: u8 }\npub struct Y { turn: Option<Turn>, item: Item, tx: Sender<u8> }\n",
    "c.rs": "pub struct Z { item: Item, tx: Sender<u8> }\nstruct Item { n: u8 }\npub struct W { item: Item, tx: Sender<u8> }\n",
}


class DuplicatedState(unittest.TestCase):
    def test_private_types_with_one_name_do_not_match_but_one_type_does(self):
        rows = depgraph.duplicated_state(depgraph.build_rust(CLASH))
        self.assertEqual([(r[1], sorted(n for n, _, _ in r[2])) for r in rows], [("item", ["W", "Z"])])

    def test_handle_types_are_not_state(self):
        src = {"h.rs": "pub struct P { mesh: Mesh, tx: Sender<u8> }\npub struct Q { mesh: Mesh, tx: Sender<u8> }\n"}
        self.assertEqual(depgraph.duplicated_state(depgraph.build_rust(src)), [])


TS = {
    "src/stores/a-state.ts": '''
import { createStore } from "zustand";
import { invoke, listen } from "../ui/tauri.ts";
import { b, setB } from "./b-state.ts";
import type { T } from "./b-state.ts";
export const a = createStore(() => ({ n: 0 }));
export function load() { invoke("cmd"); b.getState(); listen("x", () => {}); }
// setB() in a comment
''',
    "src/stores/b-state.ts": '''
import { a } from "./a-state.ts";
export const b = createStore(() => ({}));
export function setB() { a.setState({ n: 1 }); }
''',
    "src/stores/plain.ts": 'export const x = 1;\n',
    "src/ui/tauri.ts": "export function invoke() {}\n",
}


class TsStores(unittest.TestCase):
    def test_stores_edges_and_native(self):
        g = depgraph.build_ts(TS)
        self.assertEqual(set(g["nodes"]), {"src/stores/a-state.ts", "src/stores/b-state.ts", "native"})
        got = {(e["from"], e["to"], e["kind"]) for e in g["edges"]}
        self.assertIn(("src/stores/a-state.ts", "native", "order"), got)
        self.assertIn(("native", "src/stores/a-state.ts", "push"), got)
        self.assertIn(("src/stores/a-state.ts", "src/stores/b-state.ts", "ask"), got)
        self.assertIn(("src/stores/b-state.ts", "src/stores/a-state.ts", "order"), got)
        self.assertNotIn(("src/stores/a-state.ts", "src/stores/b-state.ts", "order"), got)

    def test_cycle_between_stores_and_native_loop_is_a_push(self):
        g = depgraph.build_ts(TS)
        result = depgraph.analyse(g["nodes"], g["edges"])
        self.assertEqual(len(result["cycle_groups"]), 1)
        self.assertEqual([e["from"] for e in result["upward"]], ["native"])


if __name__ == "__main__":
    unittest.main()
