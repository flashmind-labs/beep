import test from "node:test";
import assert from "node:assert/strict";
import { cachedCommandHtml, commandHtml, heredocLang, segments } from "../src/agents/command-state.ts";

test("a heredoc body takes the language of the interpreter that reads it", () => {
  assert.equal(heredocLang("python3 - "), "Python");
  assert.equal(heredocLang("cd /x && /usr/bin/python3.12 - "), "Python");
  assert.equal(heredocLang("node "), "JavaScript");
  assert.equal(heredocLang("bun run - "), "JavaScript");
  assert.equal(heredocLang("cat file | ruby "), "Ruby");
  assert.equal(heredocLang("bash "), "Shell");
});

test("with no interpreter, a heredoc takes the language of the file it is written to", () => {
  assert.equal(heredocLang("cat > src/app.tsx "), "TypeScript");
  assert.equal(heredocLang("cat >> 'a b/main.rs' "), "Rust");
  assert.equal(heredocLang("tee -a x.py "), "Python");
  assert.equal(heredocLang("cat "), "");
  assert.equal(heredocLang("cat > notes "), "");
});

test("a command splits into shell and heredoc bodies; the end line is shell", () => {
  const cmd = ["cd app", "python3 - <<'PY'", "print(1)", "PY", "echo done"].join("\n");
  assert.deepEqual(segments(cmd), [
    { lang: "Shell", lines: ["cd app", "python3 - <<'PY'"] },
    { lang: "Python", lines: ["print(1)"] },
    { lang: "Shell", lines: ["PY", "echo done"] },
  ]);
});

test("two heredocs on one line read in order; <<- ignores leading tabs; here-strings are not heredocs", () => {
  const two = ["cat <<A > a.json; cat <<B", "{}", "A", "x", "B"].join("\n");
  assert.deepEqual(segments(two).map(s => s.lang), ["Shell", "JSON", "Shell", "", "Shell"]);
  assert.deepEqual(segments("cat <<-EOF\n\tbody\n\tEOF\nls").map(s => s.lang), ["Shell", "", "Shell"]);
  assert.deepEqual(segments("grep x <<< foo\nls"), [{ lang: "Shell", lines: ["grep x <<< foo", "ls"] }]);
});

test("a heredoc with no end line runs to the end of the command", () => {
  assert.deepEqual(segments("node <<EOF\nconsole.log(1)").map(s => s.lang), ["Shell", "JavaScript"]);
});

test("the command is colored and everything is escaped", () => {
  const html = commandHtml("echo \"<b>hi</b>\" # <script>\npython3 - <<'PY'\nif x:\n    print('<img onerror=1>')\nPY");
  assert.ok(html.includes('<span class="s">&quot;&lt;b&gt;hi&lt;/b&gt;&quot;</span>'));
  assert.ok(html.includes('<span class="c"># &lt;script&gt;</span>'));
  assert.ok(html.includes('<span class="k">if</span>'));
  assert.ok(!/<(b|script|img)[ >]/.test(html));
});

test("a colored command is kept by its text", () => {
  assert.equal(cachedCommandHtml("echo 1"), cachedCommandHtml("echo 1"));
  assert.equal(cachedCommandHtml("echo 1"), commandHtml("echo 1"));
});
