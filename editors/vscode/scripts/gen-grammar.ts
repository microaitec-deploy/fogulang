// Generates syntaxes/fogu.tmLanguage.json and syntaxes/fogu.markdown.tmLanguage.json.
// Run with: bun scripts/gen-grammar.ts
//
// Token reference: lang/crates/fogu-core/src/runtime/lexer.rs and token.rs.
import { writeFileSync } from "node:fs";
import { join } from "node:path";

const r = String.raw;
const ID = r`[A-Za-z_][A-Za-z0-9_]*`;

// Keyword lists (lexer.rs read_ident + contextual words of the parser).
const CONTROL = ["if", "else", "while", "for", "in", "break", "continue", "return", "match", "case", "default", "select", "fg"];
const BUILTIN_FNS = ["print", "len", "typeof", "str", "int", "float", "bytes", "clock", "close", "timeout", "error", "panic"];
const BUILTIN_TYPES = ["int", "float", "str", "string", "bool", "void", "duration", "list", "map", "tuple", "range", "chan", "error", "fn", "struct", "object", "any", "buffer", "nil", "char",
  "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "f32", "f64",
  "int8", "int16", "int32", "int64", "uint8", "uint16", "uint32", "uint64", "uint", "float32", "float64", "byte", "rune"];
const words = (xs: string[]) => r`\b(?:` + xs.join("|") + r`)\b`;

const grammar = {
  $schema: "https://raw.githubusercontent.com/martinring/tmlanguage/master/tmlanguage.json",
  name: "Fogu",
  scopeName: "source.fogu",
  fileTypes: ["fg"],
  patterns: [{ include: "#statements" }],
  repository: {
    statements: {
      patterns: [
        { include: "#comments" },
        { include: "#import" },
        { include: "#struct" },
        { include: "#interface" },
        { include: "#variable-declaration" },
        { include: "#expression" },
      ],
    },

    expression: {
      patterns: [
        { include: "#comments" },
        { include: "#strings" },
        { include: "#numbers" },
        { include: "#function" },
        { include: "#keywords" },
        { include: "#new-expression" },
        { include: "#constants" },
        { include: "#builtins" },
        { include: "#fogu-object" },
        { include: "#function-call" },
        { include: "#property" },
        { include: "#operators" },
        { include: "#block" },
        { include: "#brackets" },
        { include: "#parens" },
        { include: "#punctuation" },
        { include: "#identifiers" },
      ],
    },

    // ----------------------------------------------------------------- comments
    comments: {
      patterns: [
        {
          name: "comment.block.documentation.fogu",
          begin: r`/\*\*(?!/)`,
          end: r`\*/`,
          beginCaptures: { 0: { name: "punctuation.definition.comment.begin.fogu" } },
          endCaptures: { 0: { name: "punctuation.definition.comment.end.fogu" } },
        },
        {
          name: "comment.block.fogu",
          begin: r`/\*`,
          end: r`\*/`,
          beginCaptures: { 0: { name: "punctuation.definition.comment.begin.fogu" } },
          endCaptures: { 0: { name: "punctuation.definition.comment.end.fogu" } },
        },
        {
          begin: r`(^[ \t]+)?(?=//)`,
          beginCaptures: { 1: { name: "punctuation.whitespace.comment.leading.fogu" } },
          end: r`(?!\G)`,
          patterns: [
            {
              name: "comment.line.double-slash.fogu",
              begin: "//",
              beginCaptures: { 0: { name: "punctuation.definition.comment.fogu" } },
              end: r`(?=$)`,
            },
          ],
        },
      ],
    },

    // ------------------------------------------------------------------ strings
    strings: {
      patterns: [{ include: "#module-specifier" }, { include: "#string-double" }, { include: "#string-single" }, { include: "#template" }],
    },
    "module-specifier": {
      comment: 'Standard library module in an import: "fogu:http"',
      match: r`(["'])(fogu)(:)([A-Za-z0-9_\-]+)(\1)`,
      name: "string.quoted.module.fogu",
      captures: {
        1: { name: "punctuation.definition.string.begin.fogu" },
        2: { name: "support.type.module.namespace.fogu" },
        3: { name: "punctuation.separator.module.fogu" },
        4: { name: "support.module.fogu entity.name.type.module.fogu" },
        5: { name: "punctuation.definition.string.end.fogu" },
      },
    },
    "string-double": {
      name: "string.quoted.double.fogu",
      begin: '"',
      end: r`"|(?=$)`,
      beginCaptures: { 0: { name: "punctuation.definition.string.begin.fogu" } },
      endCaptures: { 0: { name: "punctuation.definition.string.end.fogu" } },
      patterns: [{ include: "#string-escape" }],
    },
    "string-single": {
      name: "string.quoted.single.fogu",
      begin: "'",
      end: r`'|(?=$)`,
      beginCaptures: { 0: { name: "punctuation.definition.string.begin.fogu" } },
      endCaptures: { 0: { name: "punctuation.definition.string.end.fogu" } },
      patterns: [{ include: "#string-escape" }],
    },
    "string-escape": {
      patterns: [
        { name: "constant.character.escape.fogu", match: r`\\[ntr0\\'"]` },
        { name: "invalid.illegal.escape.fogu", match: r`\\.` },
      ],
    },
    template: {
      name: "string.template.fogu",
      begin: "`",
      end: "`",
      beginCaptures: { 0: { name: "punctuation.definition.string.template.begin.fogu" } },
      endCaptures: { 0: { name: "punctuation.definition.string.template.end.fogu" } },
      patterns: [
        { name: "constant.character.escape.fogu", match: "\\\\[ntr0\\\\`$'\"]" },
        { name: "invalid.illegal.escape.fogu", match: r`\\.` },
        { include: "#template-substitution" },
      ],
    },
    "template-substitution": {
      name: "meta.template.expression.fogu",
      begin: r`\$\{`,
      end: r`\}`,
      beginCaptures: { 0: { name: "punctuation.definition.template-expression.begin.fogu" } },
      endCaptures: { 0: { name: "punctuation.definition.template-expression.end.fogu" } },
      contentName: "meta.embedded.line.fogu",
      patterns: [{ include: "#expression" }],
    },

    // ------------------------------------------------------------------ numbers
    numbers: {
      patterns: [
        {
          name: "constant.numeric.hex.fogu",
          match: r`\b(0[xX])[0-9a-fA-F_]+\b`,
          captures: { 1: { name: "storage.type.numeric.hex.fogu" } },
        },
        {
          name: "constant.numeric.binary.fogu",
          match: r`\b(0[bB])[01_]+\b`,
          captures: { 1: { name: "storage.type.numeric.binary.fogu" } },
        },
        {
          comment: "Duration literal: 500ms 2s 1.5h 1d (ns us µs ms s m h d)",
          name: "constant.numeric.duration.fogu",
          match: r`\b[0-9][0-9_]*(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?(ns|us|µs|ms|s|m|h|d)(?![A-Za-z0-9_])`,
          captures: { 1: { name: "keyword.other.unit.duration.fogu" } },
        },
        {
          name: "constant.numeric.float.fogu",
          match: r`\b[0-9][0-9_]*(?:\.[0-9]+(?:[eE][+-]?[0-9]+)?|[eE][+-]?[0-9]+)\b`,
        },
        { name: "constant.numeric.integer.fogu", match: r`\b[0-9][0-9_]*\b` },
      ],
    },

    // ----------------------------------------------------------------- keywords
    keywords: {
      patterns: [
        { name: "keyword.control.loop.fogu", match: r`\bloop\b(?=\s*\{)` },
        { name: "keyword.control.flow.fogu", match: r`\b(?:return|break|continue)\b` },
        { name: "keyword.control.conditional.fogu", match: r`\b(?:if|else|match|case|default|select)\b` },
        { name: "keyword.control.loop.fogu", match: r`\b(?:while|for|in)\b` },
        { name: "keyword.control.concurrency.fogu", match: r`\bfg\b` },
        { name: "keyword.control.import.fogu", match: r`\b(?:import|export|from|as)\b` },
        { name: "storage.type.chan.fogu", match: r`\bchan\b` },
        { name: "storage.type.fogu", match: r`\b(?:let|const)\b` },
        { name: "storage.type.struct.fogu", match: r`\b(?:struct|interface)\b` },
        { name: "storage.modifier.embed.fogu", match: r`\bembed\b` },
        { name: "variable.language.this.fogu", match: r`\bthis\b` },
      ],
    },
    constants: {
      patterns: [
        { name: "constant.language.boolean.fogu", match: r`\b(?:true|false)\b` },
        { name: "constant.language.nil.fogu", match: r`\bnil\b` },
        { name: "variable.language.wildcard.fogu", match: r`\b_\b` },
        { name: "variable.other.constant.fogu", match: r`\b[A-Z][A-Z0-9_]*[A-Z0-9]\b(?!\s*\()` },
      ],
    },
    builtins: {
      name: "support.function.builtin.fogu",
      match: words(BUILTIN_FNS) + r`(?=\s*\()`,
    },
    "fogu-object": {
      patterns: [
        {
          match: r`\b(Fogu)\s*(\.)\s*(` + ID + r`)(?=\s*\()`,
          captures: {
            1: { name: "support.class.fogu" },
            2: { name: "punctuation.accessor.fogu" },
            3: { name: "support.function.fogu" },
          },
        },
        {
          match: r`\b(Fogu)\s*(\.)\s*(` + ID + r`)`,
          captures: {
            1: { name: "support.class.fogu" },
            2: { name: "punctuation.accessor.fogu" },
            3: { name: "support.variable.property.fogu" },
          },
        },
        { name: "support.class.fogu", match: r`\bFogu\b` },
      ],
    },

    // --------------------------------------------------------------- operators
    operators: {
      patterns: [
        { name: "keyword.operator.spread.fogu", match: r`\.\.\.` },
        { name: "keyword.operator.range.fogu", match: r`\.\.` },
        { name: "keyword.operator.optional-chain.fogu", match: r`\?\.(?!\.)` },
        { name: "keyword.operator.nullish.fogu", match: r`\?\?` },
        { name: "storage.type.function.arrow.fogu", match: "=>" },
        { name: "keyword.operator.pipe.fogu", match: "->" },
        { name: "keyword.operator.channel.fogu", match: "<-" },
        { name: "keyword.operator.assignment.compound.fogu", match: r`\+=|-=|\*=|/=|%=` },
        { name: "keyword.operator.comparison.fogu", match: r`==|!=` },
        { name: "keyword.operator.relational.fogu", match: r`<=|>=|<|>` },
        { name: "keyword.operator.logical.fogu", match: r`&&|\|\||!` },
        { name: "keyword.operator.assignment.fogu", match: "=" },
        { name: "keyword.operator.arithmetic.fogu", match: r`\*\*|[+\-*/%]` },
        { name: "keyword.operator.error-propagation.fogu", match: r`\?` },
        { name: "keyword.operator.alternative.fogu", match: r`\|` },
      ],
    },
    punctuation: {
      patterns: [
        { name: "punctuation.separator.comma.fogu", match: "," },
        { name: "punctuation.terminator.statement.fogu", match: ";" },
        { name: "punctuation.separator.key-value.fogu", match: ":" },
        { name: "punctuation.accessor.fogu", match: r`\.` },
      ],
    },
    block: {
      begin: r`\{`,
      end: r`\}`,
      beginCaptures: { 0: { name: "punctuation.definition.block.begin.fogu" } },
      endCaptures: { 0: { name: "punctuation.definition.block.end.fogu" } },
      patterns: [
        { include: "#map-key" },
        { include: "#statements" },
      ],
    },
    "map-key": {
      comment: "Key of a map literal or struct field: `name:` at the start of an entry",
      match: r`(?<=[{,]|^)\s*(?!(?:case|default)\b)(` + ID + r`)\s*(:)(?!:)`,
      captures: {
        1: { name: "meta.object-literal.key.fogu variable.other.property.fogu" },
        2: { name: "punctuation.separator.key-value.fogu" },
      },
    },
    brackets: {
      begin: r`\[`,
      end: r`\]`,
      beginCaptures: { 0: { name: "punctuation.definition.brackets.begin.fogu" } },
      endCaptures: { 0: { name: "punctuation.definition.brackets.end.fogu" } },
      patterns: [{ include: "#expression" }],
    },
    parens: {
      begin: r`\(`,
      end: r`\)`,
      beginCaptures: { 0: { name: "punctuation.definition.parens.begin.fogu" } },
      endCaptures: { 0: { name: "punctuation.definition.parens.end.fogu" } },
      patterns: [{ include: "#expression" }],
    },

    // ----------------------------------------------------------- declarations
    import: {
      name: "meta.import.fogu",
      begin: r`\b(import)\b`,
      beginCaptures: { 1: { name: "keyword.control.import.fogu" } },
      end: r`(?<=["'])|(?=;|$)`,
      patterns: [
        { include: "#comments" },
        { name: "keyword.control.import.fogu", match: r`\b(?:from|as)\b` },
        { name: "keyword.operator.wildcard.fogu", match: r`\*` },
        {
          begin: r`\{`,
          end: r`\}`,
          beginCaptures: { 0: { name: "punctuation.definition.block.begin.fogu" } },
          endCaptures: { 0: { name: "punctuation.definition.block.end.fogu" } },
          patterns: [
            { name: "keyword.control.import.fogu", match: r`\bas\b` },
            { name: "variable.other.readwrite.alias.fogu", match: ID },
            { name: "punctuation.separator.comma.fogu", match: "," },
          ],
        },
        { include: "#strings" },
        { name: "variable.other.readwrite.alias.fogu", match: ID },
      ],
    },
    "variable-declaration": {
      patterns: [
        {
          comment: "let x: type = …",
          begin: r`\b(let|const)\s+(` + ID + r`)\s*(:)`,
          beginCaptures: {
            1: { name: "storage.type.fogu" },
            2: { name: "variable.other.declaration.fogu" },
            3: { name: "punctuation.separator.type.fogu" },
          },
          end: r`(?==|;|$)`,
          patterns: [{ include: "#type" }],
        },
        {
          match: r`\b(let|const)\s+(` + ID + r`)`,
          captures: {
            1: { name: "storage.type.fogu" },
            2: { name: "variable.other.declaration.fogu" },
          },
        },
      ],
    },
    function: {
      comment: "fn name<T>(params): type — also anonymous functions and fn types",
      name: "meta.function.fogu",
      begin: r`\b(fn)\b(?:\s*(` + ID + r`))?\s*(<[^<>()]*>)?`,
      beginCaptures: {
        1: { name: "storage.type.function.fogu" },
        2: { name: "entity.name.function.fogu" },
        3: { patterns: [{ include: "#type-parameters" }] },
      },
      end: r`(?=[{},;\]=)])|(?<=\))(?!\s*:)|$`,
      patterns: [{ include: "#comments" }, { include: "#parameters" }, { include: "#return-type" }],
    },
    "type-parameters": {
      patterns: [
        { name: "punctuation.definition.typeparameters.fogu", match: "[<>]" },
        { name: "punctuation.separator.comma.fogu", match: "," },
        { name: "punctuation.separator.type.fogu", match: ":" },
        { name: "entity.name.type.parameter.fogu", match: ID },
      ],
    },
    parameters: {
      name: "meta.parameters.fogu",
      begin: r`\(`,
      end: r`\)`,
      beginCaptures: { 0: { name: "punctuation.definition.parameters.begin.fogu" } },
      endCaptures: { 0: { name: "punctuation.definition.parameters.end.fogu" } },
      patterns: [
        { include: "#comments" },
        {
          comment: "name: type",
          begin: r`(\.\.\.)?(` + ID + r`)\s*(:)`,
          beginCaptures: {
            1: { name: "keyword.operator.spread.fogu" },
            2: { name: "variable.parameter.fogu" },
            3: { name: "punctuation.separator.type.fogu" },
          },
          end: r`(?=[,=)])`,
          patterns: [{ include: "#type" }],
        },
        {
          comment: "= default value",
          begin: "=",
          beginCaptures: { 0: { name: "keyword.operator.assignment.fogu" } },
          end: r`(?=[,)])`,
          patterns: [{ include: "#expression" }],
        },
        {
          match: r`(\.\.\.)?(` + ID + r`)`,
          captures: {
            1: { name: "keyword.operator.spread.fogu" },
            2: { name: "variable.parameter.fogu" },
          },
        },
        { name: "punctuation.separator.comma.fogu", match: "," },
      ],
    },
    "return-type": {
      begin: r`(?<=\))\s*(:)`,
      beginCaptures: { 1: { name: "punctuation.separator.type.fogu" } },
      end: r`(?=[{},;\]=)])|$`,
      patterns: [{ include: "#type" }],
    },
    type: {
      patterns: [
        { include: "#comments" },
        { include: "#function" },
        { name: "support.type.primitive.fogu", match: words(BUILTIN_TYPES.filter((t) => t !== "fn")) },
        {
          begin: "<",
          end: ">",
          beginCaptures: { 0: { name: "punctuation.definition.typeparameters.begin.fogu" } },
          endCaptures: { 0: { name: "punctuation.definition.typeparameters.end.fogu" } },
          patterns: [{ include: "#type" }, { name: "punctuation.separator.comma.fogu", match: "," }],
        },
        {
          begin: r`\(`,
          end: r`\)`,
          beginCaptures: { 0: { name: "meta.brace.round.fogu" } },
          endCaptures: { 0: { name: "meta.brace.round.fogu" } },
          patterns: [{ include: "#type" }, { name: "punctuation.separator.comma.fogu", match: "," }],
        },
        { name: "keyword.operator.type.fogu", match: r`\|\||[|?]` },
        { name: "entity.name.type.fogu", match: ID },
      ],
    },
    struct: {
      name: "meta.struct.fogu",
      begin: r`\b(export\s+)?(struct)\s+(` + ID + r`)\s*(<[^<>]*>)?\s*`,
      beginCaptures: {
        1: { name: "keyword.control.export.fogu" },
        2: { name: "storage.type.struct.fogu" },
        3: { name: "entity.name.type.struct.fogu" },
        4: { patterns: [{ include: "#type-parameters" }] },
      },
      end: r`(?<=\})`,
      patterns: [{ include: "#comments" }, { include: "#struct-body" }],
    },
    interface: {
      name: "meta.interface.fogu",
      begin: r`\b(export\s+)?(interface)\s+(` + ID + r`)\s*(<[^<>]*>)?\s*`,
      beginCaptures: {
        1: { name: "keyword.control.export.fogu" },
        2: { name: "storage.type.interface.fogu" },
        3: { name: "entity.name.type.interface.fogu" },
        4: { patterns: [{ include: "#type-parameters" }] },
      },
      end: r`(?<=\})`,
      patterns: [{ include: "#comments" }, { include: "#struct-body" }],
    },
    "struct-body": {
      begin: r`\{`,
      end: r`\}`,
      beginCaptures: { 0: { name: "punctuation.definition.block.begin.fogu" } },
      endCaptures: { 0: { name: "punctuation.definition.block.end.fogu" } },
      patterns: [
        { include: "#comments" },
        {
          match: r`\b(embed)\s+(` + ID + r`)`,
          captures: { 1: { name: "storage.modifier.embed.fogu" }, 2: { name: "entity.name.type.fogu" } },
        },
        {
          comment: "constructor new(...) { }",
          begin: r`(?<![.\w])(new)\s*(?=\()`,
          beginCaptures: { 1: { name: "entity.name.function.constructor.fogu" } },
          end: r`(?=\{)|(?<=\))(?!\s*:)|$`,
          patterns: [{ include: "#parameters" }, { include: "#return-type" }],
        },
        { include: "#function" },
        {
          comment: "field: type = default",
          begin: r`^\s*(` + ID + r`)\s*(:)`,
          beginCaptures: {
            1: { name: "variable.other.property.fogu" },
            2: { name: "punctuation.separator.type.fogu" },
          },
          end: r`(?==|;|,|\}|$)`,
          patterns: [{ include: "#type" }],
        },
        {
          match: r`^\s*(` + ID + r`)\s*(?==)`,
          captures: { 1: { name: "variable.other.property.fogu" } },
        },
        { include: "#statements" },
      ],
    },
    "new-expression": {
      match: r`\b(new)\s+(` + ID + r`)`,
      captures: {
        1: { name: "keyword.operator.new.fogu" },
        2: { name: "entity.name.type.fogu" },
      },
    },

    // ------------------------------------------------------- calls and names
    "function-call": {
      match: r`(` + ID + r`)\s*(?=\()`,
      captures: { 1: { name: "entity.name.function.call.fogu" } },
    },
    property: {
      patterns: [
        {
          match: r`(?:(\?\.)|(\.))(?!\.)\s*(` + ID + r`)\s*(?=\()`,
          captures: {
            1: { name: "keyword.operator.optional-chain.fogu" },
            2: { name: "punctuation.accessor.fogu" },
            3: { name: "entity.name.function.method.fogu" },
          },
        },
        {
          match: r`(?:(\?\.)|(\.))(?!\.)\s*(` + ID + r`)`,
          captures: {
            1: { name: "keyword.operator.optional-chain.fogu" },
            2: { name: "punctuation.accessor.fogu" },
            3: { name: "variable.other.property.fogu" },
          },
        },
      ],
    },
    identifiers: {
      name: "variable.other.readwrite.fogu",
      match: ID,
    },
  },
};

const markdown = {
  $schema: "https://raw.githubusercontent.com/martinring/tmlanguage/master/tmlanguage.json",
  scopeName: "markdown.fogu.codeblock",
  injectionSelector: "L:text.html.markdown",
  patterns: [{ include: "#fogu-code-block" }],
  repository: {
    "fogu-code-block": {
      begin: r`(^|\G)(\s*)(\`{3,}|~{3,})\s*(?i:(fogu|fg)((\s+|:|,|\{|\?)[^\`]*)?$)`,
      name: "markup.fenced_code.block.markdown",
      end: r`(^|\G)(\2|\s{0,3})(\3)\s*$`,
      beginCaptures: {
        3: { name: "punctuation.definition.markdown" },
        4: { name: "fenced_code.block.language.markdown" },
        5: { name: "fenced_code.block.language.attributes.markdown" },
      },
      endCaptures: { 3: { name: "punctuation.definition.markdown" } },
      patterns: [
        {
          begin: r`(^|\G)(\s*)(.*)`,
          while: r`(^|\G)(?!\s*([\`~]{3,})\s*$)`,
          contentName: "meta.embedded.block.fogu",
          patterns: [{ include: "source.fogu" }],
        },
      ],
    },
  },
};

const out = join(import.meta.dir, "..", "syntaxes");
writeFileSync(join(out, "fogu.tmLanguage.json"), JSON.stringify(grammar, null, 2) + "\n");
writeFileSync(join(out, "fogu.markdown.tmLanguage.json"), JSON.stringify(markdown, null, 2) + "\n");
console.log("wrote syntaxes/fogu.tmLanguage.json and syntaxes/fogu.markdown.tmLanguage.json");
