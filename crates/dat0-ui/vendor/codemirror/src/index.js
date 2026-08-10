// dat0's CodeMirror 6 bundle.
//
// Built to `crates/dat0-ui/assets/codemirror.js` by `node build.mjs` and served
// from the binary over the `dat0` custom asset protocol. Cargo never runs this
// build — see ../README.md.
//
// Exposes exactly one global, `window.dat0cm`, which the Rust side drives
// through `document::eval`. The protocol is the one specified in Phase 4.2 of
// the migration plan:
//
//   Rust -> JS   {t:"init",    id, doc, schema:{table:[col,...]}, functions:[...], vars:{...}}
//                {t:"set_doc", id, doc}
//                {t:"focus",   id}
//                {t:"theme",   id, vars:{...}}
//   JS -> Rust   {t:"ready",   id}
//                {t:"change",  id, doc}
//                {t:"cursor",  id, line, col}
//                {t:"run",     id, doc}
//
// The editor owns its own keymap, so Cmd/Ctrl-Enter is bound *inside*
// CodeMirror and surfaces as a `run` message rather than escaping to the
// shell's key cascade.

import { EditorState, Compartment } from "@codemirror/state";
import {
  EditorView,
  keymap,
  lineNumbers,
  highlightActiveLine,
  highlightActiveLineGutter,
  drawSelection,
  rectangularSelection,
  crosshairCursor,
  highlightSpecialChars,
} from "@codemirror/view";
import {
  defaultKeymap,
  history,
  historyKeymap,
  indentWithTab,
} from "@codemirror/commands";
import {
  autocompletion,
  completionKeymap,
  closeBrackets,
  closeBracketsKeymap,
  completionStatus,
  currentCompletions,
  startCompletion,
} from "@codemirror/autocomplete";
import { searchKeymap, highlightSelectionMatches } from "@codemirror/search";
import { sql, PostgreSQL } from "@codemirror/lang-sql";
import {
  syntaxHighlighting,
  HighlightStyle,
  bracketMatching,
  indentOnInput,
} from "@codemirror/language";
import { tags } from "@lezer/highlight";

/** Mounted editors, keyed by the id Rust assigns per console tab. */
const editors = new Map();

/**
 * The JS -> Rust channel.
 *
 * `document::eval` hands the script a *scoped* `dioxus` object; it is not
 * published on `window`, so a module loaded via `<script src>` cannot reach it.
 * Rust therefore opens one long-lived eval per window and its first act is
 * `dat0cm.bind(dioxus)`. Until that happens `notify` is a no-op rather than a
 * crash, so `init` can be handled before the channel exists.
 */
let channel = null;

/**
 * The theme is generated from dat0's own `--d0-*` tokens rather than a stock
 * CodeMirror theme, so the editor cannot drift from the rest of the app.
 * `vars` carries the *resolved* values (Rust reads them out of `ThemeTokens`).
 */
function buildTheme(vars) {
  const v = (name, fallback) => vars[name] || fallback;
  const theme = EditorView.theme(
    {
      "&": {
        color: v("fg", "#1f2328"),
        backgroundColor: v("surface", "#ffffff"),
        fontFamily: "'Geist Mono', ui-monospace, SFMono-Regular, Menlo, monospace",
        fontSize: "12.5px",
        height: "100%",
      },
      // The family is repeated here, not just on `&`: CodeMirror ships its own
      // `.cm-content { font-family: monospace }`, which wins over an inherited
      // value and silently renders the whole editor in the system mono font.
      ".cm-content": {
        fontFamily: "'Geist Mono', ui-monospace, SFMono-Regular, Menlo, monospace",
        fontSize: "12.5px",
        caretColor: v("accent", "#03459b"),
        lineHeight: "2",
        padding: "0 16px",
      },
      ".cm-cursor, .cm-dropCursor": { borderLeftColor: v("accent", "#03459b") },
      "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection":
        { backgroundColor: v("activeBg", "#e8f1fb") },
      ".cm-activeLine": { backgroundColor: v("rowHover", "#eef1f4") },
      ".cm-gutters": {
        fontFamily: "'Geist Mono', ui-monospace, SFMono-Regular, Menlo, monospace",
        backgroundColor: v("paneHead", "#f7f8fa"),
        color: v("muted", "#4f5760"),
        border: "none",
        borderRight: "1px solid " + v("ruleDim", "#dde2e8"),
        minWidth: "44px",
        fontSize: "11px",
        letterSpacing: "0.06em",
      },
      ".cm-activeLineGutter": {
        backgroundColor: v("paneHead", "#f7f8fa"),
        color: v("fg", "#1f2328"),
      },
      ".cm-tooltip": {
        backgroundColor: v("surface", "#ffffff"),
        border: "1px solid " + v("rule", "#d0d7de"),
        borderRadius: "5px",
        boxShadow: v("shadowOverlay", "0 24px 64px rgba(31,35,40,0.18)"),
      },
      ".cm-tooltip-autocomplete > ul > li": {
        fontFamily: "'Geist Mono', ui-monospace, monospace",
        fontSize: "12.5px",
        padding: "2px 8px",
      },
      ".cm-tooltip-autocomplete > ul > li[aria-selected]": {
        backgroundColor: v("activeBg", "#e8f1fb"),
        color: v("ink", "#0a0b0d"),
      },
    },
    { dark: vars.mode === "dark" },
  );

  const highlight = HighlightStyle.define([
    { tag: tags.keyword, color: v("sqlKeyword", "#5a32a3") },
    { tag: [tags.number, tags.bool, tags.null], color: v("sqlNumber", "#6f42c1") },
    { tag: [tags.string, tags.special(tags.string)], color: v("sqlString", "#116329") },
    { tag: [tags.function(tags.variableName), tags.standard(tags.variableName)], color: v("sqlFn", "#03459b") },
    { tag: [tags.comment, tags.lineComment, tags.blockComment], color: v("sqlComment", "#4f5760"), fontStyle: "italic" },
    { tag: tags.operator, color: v("fg", "#1f2328") },
  ]);

  return [theme, syntaxHighlighting(highlight)];
}

/**
 * Static completions for DuckDB's function catalogue, supplied by Rust.
 * `@codemirror/lang-sql` already handles schema-qualified table/column
 * completion; this only adds the function names, registered as an extra
 * completion source on the SQL language rather than an `override` (which would
 * replace lang-sql's own sources).
 */
function functionCompletionSource(support, functions) {
  if (!functions || functions.length === 0) return [];
  const options = functions.map((label) => ({ label, type: "function", boost: -1 }));
  return support.language.data.of({
    autocomplete: (ctx) => {
      const word = ctx.matchBefore(/\w*/);
      if (!word || (word.from === word.to && !ctx.explicit)) return null;
      return { from: word.from, options, validFor: /^\w*$/ };
    },
  });
}

function mount(opts) {
  const { id, doc, schema, functions, vars, parent } = opts;
  const themeC = new Compartment();

  const notify = (msg) => {
    if (channel && channel.send) channel.send(msg);
  };

  const runCommand = (view) => {
    notify({ t: "run", id, doc: view.state.doc.toString() });
    return true;
  };

  const sqlSupport = sql({
    dialect: PostgreSQL,
    schema: schema || {},
    upperCaseKeywords: true,
  });
  const state = EditorState.create({
    doc: doc || "",
    extensions: [
      lineNumbers(),
      highlightActiveLineGutter(),
      highlightSpecialChars(),
      history(),
      drawSelection(),
      indentOnInput(),
      bracketMatching(),
      closeBrackets(),
      autocompletion({ activateOnTyping: true, defaultKeymap: true }),
      rectangularSelection(),
      crosshairCursor(),
      highlightActiveLine(),
      highlightSelectionMatches(),
      keymap.of([
        { key: "Mod-Enter", run: runCommand, preventDefault: true },
        ...closeBracketsKeymap,
        ...defaultKeymap,
        ...searchKeymap,
        ...historyKeymap,
        ...completionKeymap,
        // The way out, by keyboard.
        //
        // `indentWithTab` below is what makes Tab indent instead of moving
        // focus — which is a keyboard trap unless there is an escape hatch.
        // gpui-component's `Input` had the same trap and `view/sql_console.rs`
        // ended its Escape ladder on exactly this rung: Escape from the editor
        // lands on the Run control.
        //
        // Last in the list on purpose. Within one keymap the earlier binding
        // wins, and both `completionKeymap` and `searchKeymap` bind Escape to
        // close their own overlay — each returns false when it has nothing
        // open, so this only fires when Escape means "let me out".
        {
          key: "Escape",
          run: () => {
            const out = document.querySelector(
              '[data-a11y-id="console-run"], [data-a11y-id="console-cancel"]',
            );
            if (!out) return false;
            out.focus();
            return true;
          },
        },
        indentWithTab,
      ]),
      sqlSupport,
      functionCompletionSource(sqlSupport, functions),
      themeC.of(buildTheme(vars || {})),
      EditorView.updateListener.of((u) => {
        if (u.docChanged) {
          notify({ t: "change", id, doc: u.state.doc.toString() });
        }
        if (u.selectionSet) {
          const head = u.state.selection.main.head;
          const line = u.state.doc.lineAt(head);
          notify({ t: "cursor", id, line: line.number, col: head - line.from + 1 });
        }
      }),
    ],
  });

  const view = new EditorView({ state, parent });
  editors.set(id, { view, themeC });
  notify({ t: "ready", id });
  return view;
}

window.dat0cm = {
  /**
   * Bind the JS -> Rust channel. Rust calls this once per window, from the
   * long-lived `document::eval` whose scoped `dioxus` object is the only way
   * back to Rust.
   */
  bind(ch) {
    channel = ch;
    return true;
  },

  /** Handle one Rust -> JS message. */
  handle(msg) {
    switch (msg.t) {
      case "init": {
        const parent = document.getElementById(msg.mount || "cm-" + msg.id);
        if (!parent) return false;
        parent.innerHTML = "";
        mount({ ...msg, parent });
        return true;
      }
      case "set_doc": {
        const e = editors.get(msg.id);
        if (!e) return false;
        e.view.dispatch({
          changes: { from: 0, to: e.view.state.doc.length, insert: msg.doc },
        });
        return true;
      }
      case "focus": {
        const e = editors.get(msg.id);
        if (!e) return false;
        e.view.focus();
        return true;
      }
      case "theme": {
        const e = editors.get(msg.id);
        if (!e) return false;
        e.view.dispatch({ effects: e.themeC.reconfigure(buildTheme(msg.vars || {})) });
        return true;
      }
      default:
        return false;
    }
  },

  /** Test seam: type text into an editor as if the user had. */
  type(id, text) {
    const e = editors.get(id);
    if (!e) return false;
    const view = e.view;
    view.focus();
    const pos = view.state.doc.length;
    view.dispatch({
      changes: { from: pos, insert: text },
      selection: { anchor: pos + text.length },
      userEvent: "input.type",
    });
    return true;
  },

  /** Test seam: force the completion popup open. */
  complete(id) {
    const e = editors.get(id);
    if (!e) return false;
    return startCompletion(e.view);
  },

  /** Test seam: the labels currently offered by the completion popup. */
  completions(id) {
    const e = editors.get(id);
    if (!e) return null;
    if (completionStatus(e.view.state) !== "active") return null;
    return currentCompletions(e.view.state).map((c) => c.label);
  },

  /**
   * Test seam: dispatch a real `keydown` at the editor's content element, so
   * the assertion exercises CodeMirror's own keymap rather than calling the
   * command directly. Used to prove Mod-Enter surfaces as a `run` message.
   */
  key(id, key, mods) {
    const e = editors.get(id);
    if (!e) return false;
    const m = mods || {};
    return !e.view.contentDOM.dispatchEvent(
      new KeyboardEvent("keydown", {
        key,
        code: key === "Enter" ? "Enter" : "Key" + key.toUpperCase(),
        metaKey: !!m.meta,
        ctrlKey: !!m.ctrl,
        shiftKey: !!m.shift,
        altKey: !!m.alt,
        bubbles: true,
        cancelable: true,
      }),
    );
  },

  doc(id) {
    const e = editors.get(id);
    return e ? e.view.state.doc.toString() : null;
  },
};
