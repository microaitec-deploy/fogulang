# Grammar (EBNF)

This grammar is derived from the reference implementation, not from the
specification: `lexer.rs`, `token.rs`, `parser.rs` and `ast.rs` in
`lang/crates/fogu-core/src/runtime/`. It describes what the parser accepts
(Fogu 0.8.4). Constructs the parser does not accept are listed at the end
under "Not part of the grammar".

## Notation

The grammar uses ISO-style EBNF with these conventions: `name = body ;`
defines a production; `|` separates alternatives; `[ x ]` means x is optional;
`{ x }` means x repeated zero or more times; `( x )` groups; terminals are
written in double quotes (a double quote itself is written `'"'`);
`/* ... */` is a comment, also used for constraints that EBNF cannot express.
Lexical productions (section 1) operate on characters; all later productions
operate on tokens, with whitespace and comments already removed.

## 1. Lexical structure

```ebnf
source        = [ BOM ] { whitespace | comment | token } ;
BOM           = /* U+FEFF, a leading byte order mark: dropped */ ;

whitespace    = /* any Unicode whitespace character, including line breaks */ ;
comment       = line_comment | block_comment ;
line_comment  = "//" { /* any char except LF, CR, U+2028, U+2029 */ } ;
block_comment = "/*" { /* any char */ } "*/" ;
               /* not nested; an unterminated block comment is an error */

token         = keyword | IDENT | "_" | INT | FLOAT | DURATION
              | STRING | TEMPLATE | operator | delimiter ;

/* ---- identifiers and keywords ---- */

IDENT         = ident_start { ident_char } ;
               /* except the keywords, and except "_" alone */
ident_start   = letter | "_" ;
ident_char    = letter | unicode_digit | "_" ;
letter        = /* Unicode alphabetic character */ ;
unicode_digit = /* Unicode numeric character (char::is_alphanumeric) */ ;
               /* "_" not followed by an ident_char is the token "_" (wildcard) */

keyword       = "let" | "const" | "fn" | "return" | "if" | "else" | "while"
              | "for" | "in" | "break" | "continue" | "true" | "false" | "nil"
              | "import" | "export" | "from" | "as" | "struct" | "interface"
              | "embed" | "match" | "case" | "default" | "fg" | "chan"
              | "select" ;

/* Contextual words (ordinary identifiers to the lexer):
   "loop" (statement start, before "{"), "new" (before an identifier),
   "any" (in a type). */

/* ---- numbers ---- */

INT           = dec_digits | hex_int | bin_int ;
hex_int       = "0" ( "x" | "X" ) hex_part { hex_part } ;
bin_int       = "0" ( "b" | "B" ) bin_part { bin_part } ;
hex_part      = hex_digit | "_" ;   /* at least one real digit is required */
bin_part      = "0" | "1" | "_" ;   /* at least one real digit is required */
dec_digits    = digit { digit | "_" } ;
FLOAT         = dec_digits "." digit { digit | "_" } [ exponent ]
              | dec_digits exponent ;
               /* "." starts a fraction only when a digit follows it */
exponent      = ( "e" | "E" ) [ "+" | "-" ] digit { digit } ;
DURATION      = ( dec_digits | FLOAT ) duration_unit ;
duration_unit = "ns" | "us" | "µs" | "ms" | "s" | "m" | "h" | "d" ;
               /* only when the whole run of ident_char after the number is
                  exactly one unit; not available after hex_int / bin_int */
digit         = "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" ;
hex_digit     = digit | "a" | "b" | "c" | "d" | "e" | "f"
              | "A" | "B" | "C" | "D" | "E" | "F" ;

/* ---- strings ---- */

STRING        = '"' { str_char | escape } '"'
              | "'" { str_char | escape } "'" ;
               /* both quote styles produce the same string token; there is no
                  character literal; a raw line break inside is allowed */
str_char      = /* any char except "\" and the closing quote */ ;
escape        = "\" ( "n" | "t" | "r" | "0" | "\" | "'" | '"' ) ;

TEMPLATE      = "`" { tpl_char | tpl_escape | interpolation } "`" ;
tpl_char      = /* any char except "`", "\", and "$" when followed by "{" */ ;
tpl_escape    = "\" ( "n" | "t" | "r" | "0" | "\" | "`" | "$" | "'" | '"' ) ;
interpolation = "${" expr "}" ;
               /* the text up to the matching "}" (nested braces and quoted
                  strings are skipped) is lexed on its own and must be exactly
                  one non-empty expr */

/* ---- operators and delimiters ---- */

operator      = "+" | "-" | "*" | "/" | "%" | "**"
              | "=" | "==" | "!" | "!=" | "<" | "<=" | ">" | ">=" | "<-"
              | "&&" | "||" | "|" | "&" | "^" | "~" | "<<" | ">>"
              | "->" | "=>" | "?" | "??" | "?."
              | "+=" | "-=" | "*=" | "/=" | "%=" | "&=" | "|=" | "^="
              | "<<=" | ">>=" ;
               /* longest match; "?." is not formed when another "." follows
                  ("x?..y" is "?" then "..") */
delimiter     = "(" | ")" | "{" | "}" | "[" | "]"
              | "," | ";" | ":" | "." | ".." | "..." ;

/* ---- statement termination ---- */

/* There is no newline token. A statement ends where its production ends;
   an optional ";" may follow. Line breaks matter only in the few places
   listed in the Notes (they are detected by comparing token line numbers). */
```

```fogu
// line comment
/* block comment */
let n = 1_000 + 0xFF + 0b1010; let f = 2.5e-3
let d = 500ms + 1.5s
let s = "a\tb" + 'single' + `sum ${n + 1} at ${d}`
print(n, f, d, s)
```

## 2. Program and modules

```ebnf
program       = { ";" } { statement { ";" } } EOF ;

import_stmt   = "import" import_clause "from" STRING [ ";" ] ;
               /* only when "import" is not followed by "(" */
import_clause = import_names
              | "*" "as" IDENT
              | IDENT [ "," ( import_names | "*" "as" IDENT ) ] ;
import_names  = "{" [ import_name { "," import_name } [ "," ] ] "}" ;
import_name   = member_name [ "as" IDENT ] ;
               /* "default" must be renamed: { default as name } */
member_name   = IDENT | "default" ;

export_stmt   = "export" export_item ;
export_item   = fn_decl
              | let_decl
              | struct_decl
              | interface_decl
              | export_names [ "from" STRING ] [ ";" ]
              | "*" [ "as" member_name ] "from" STRING [ ";" ]
              | "default" ( fn_decl | struct_decl | expr [ ";" ] ) ;
               /* "default fn" is a declaration only when a name follows "fn" */
export_names  = "{" [ export_name { "," export_name } [ "," ] ] "}" ;
export_name   = member_name [ "as" member_name ] ;
               /* without "from", the left-hand name may not be "default" */

dyn_import    = "import" "(" expr ")" ;      /* an expression, see section 6 */
```

```fogu
import math from "fogu:math"
import { floor, sqrt as root } from "fogu:math"
import * as m from "fogu:math"
export fn area(r) { return math.PI * r * r }
export const origin = [floor(0.5), root(4), m.abs(-1)]
export { area as default }
```

## 3. Declarations

```ebnf
let_decl      = ( "let" | "const" ) let_target "=" expr [ ";" ] ;
               /* "let" is mutable, "const" is not; the initializer is required */
let_target    = IDENT [ ":" type ]
              | "[" let_name { "," let_name } "]" ;   /* no trailing comma */
let_name      = IDENT | "_" ;

fn_decl       = "fn" IDENT fn_tail ;
fn_tail       = [ type_params ] param_list [ ":" type ] block ;
param_list    = "(" [ param { "," param } [ "," ] ] ")" ;
param         = [ "..." ] IDENT [ ":" type ] [ "=" expr ] ;
               /* "..." only on the last parameter and never with a default;
                  parameter names must be distinct */
type_params   = "<" type_param { "," type_param } ">" ;
type_param    = IDENT [ ":" type ] ;          /* the bound is parsed and ignored */

struct_decl   = "struct" IDENT [ type_params ] "{" { struct_member } "}" ;
struct_member = "embed" IDENT                         /* no ";" allowed after it */
              | "fn" IDENT fn_tail                    /* method */
              | IDENT param_list [ ":" type ] block   /* method without "fn" */
              | IDENT [ ":" type ] [ "=" expr ] [ ";" ] ;   /* field */
               /* a method named "new" is the constructor; members are not
                  separated by commas */

interface_decl   = "interface" IDENT [ type_params ]
                   "{" { interface_member [ ";" | "," ] } "}" ;
interface_member = "embed" IDENT
                 | [ "fn" ] IDENT ":" type                         /* field */
                 | [ "fn" ] IDENT "(" [ iface_param { "," iface_param } [ "," ] ] ")"
                   [ ":" type ] ;                                  /* method */
iface_param      = [ "..." ] IDENT [ ":" type ] ;
```

```fogu
let count: int = 0
const [first, _] = [1, 2]
fn add<T>(a: T, b: T = 1, ...rest: int): T { return a + b }
struct Base { id = 0 }
struct Point<T> { embed Base
  x: int = 0; tag: T
  new(x) { this.x = x }
  fn norm(): int { return this.x } }
interface Named { fn label(...parts) }
interface Shape { area(): float; name: str, embed Named }
print(count, first, add(1), new Point<str>(3).norm())
```

## 4. Types

Type annotations appear after `:` in `let`/`const`, parameters, return types,
struct fields and interface members.

```ebnf
type          = type_postfix { ( "|" | "||" ) type_postfix } ;     /* union */
type_postfix  = type_atom { "?" } ;                                /* optional */
type_atom     = "(" type { "," type } ")"    /* one item: grouping; more: tuple */
              | "nil"
              | "fn" [ "(" [ type { "," type } [ "," ] ] ")" [ ":" type ] ]
              | "chan" [ type_args ]
              | IDENT [ type_args ] ;
type_args     = "<" type { "," type } ">" ;
               /* a ">>" or ">>=" token closing two lists is split by the parser */

/* Type names are plain identifiers: "int", "float", "str", "bool", "list",
   "map", "error", sized names such as "u8", "i64", "f32", struct and
   interface names are all the IDENT alternative. "any" and the type
   parameters in scope are erased to "any". */
```

```fogu
let a: int? = nil
let b: int | str = "x"
let c: map<str, list<list<int>>> = {k: [[1]]}
let f: fn(int, int): int = (x, y) => x + y
fn pair(): (int, error) { return 1, nil }
print(a, b, c, f(1, 2), pair())
```

## 5. Statements

```ebnf
statement     = let_decl
              | fn_decl              /* "fn" followed by an identifier */
              | struct_decl
              | interface_decl
              | import_stmt
              | export_stmt
              | if_stmt
              | while_stmt
              | loop_stmt
              | for_stmt
              | match_stmt
              | select_stmt
              | return_stmt
              | "break" [ ";" ]
              | "continue" [ ";" ]
              | block                /* "{" at statement start is always a block */
              | send_stmt
              | expr_stmt ;

block         = "{" { ";" } { statement { ";" } } "}" ;

if_stmt       = "if" expr block [ "else" ( if_stmt | block ) ] ;
while_stmt    = "while" expr block ;
loop_stmt     = "loop" block ;                /* same as: while true block */
for_stmt      = "for" IDENT [ "," IDENT ] "in" expr block ;

match_stmt    = "match" expr match_body ;     /* match_body: section 7 */

return_stmt   = "return" [ expr { "," expr } ] [ ";" ] ;
               /* no value when ";", "}", end of file or a line break follows
                  "return"; several values form a tuple */

send_stmt     = expr "<-" expr [ ";" ] ;      /* "<-" on the same line as the
                                                 end of the left expression */
expr_stmt     = expr [ ";" ] ;                /* includes assignments */

select_stmt   = "select" "{" { select_case } "}" ;
select_case   = "case" "<-" unary ":" case_body               /* receive, discard */
              | "case" IDENT "=" "<-" unary ":" case_body     /* receive, bind */
              | "case" expr "<-" expr ":" case_body           /* send */
              | "default" ":" case_body ;
case_body     = { statement } ;   /* up to the next "case", "default" or "}" */

/* Assignment is an expression (section 6); as a statement it has the forms */
assignment    = assign_target assign_op expr ;
assign_target = IDENT | postfix "." field_name | postfix "[" expr "]" ;
assign_op     = "=" | "+=" | "-=" | "*=" | "/=" | "%="
              | "&=" | "|=" | "^=" | "<<=" | ">>=" ;
```

```fogu
let total = 0
for i, v in [1, 2, 3] { if v % 2 == 0 { continue } else { total += v + i } }
while total > 100 { total >>= 1 }
loop { break }
let c = chan(1); c <- total
select { case v = <-c: print(v) default: print("empty") }
```

## 6. Expressions

Precedence, from lowest (binds loosest) to highest. Each level is one parser
function, and each function calls the next one for its operands.

| Level | Parser function    | Operators                        | Associativity          |
|-------|--------------------|----------------------------------|------------------------|
| 1     | `parse_assignment` | `=` `+=` `-=` `*=` `/=` `%=` `&=` `\|=` `^=` `<<=` `>>=` | right |
| 2     | `parse_range`      | `..`                             | none (at most one)     |
| 3     | `parse_or`         | `\|\|` `??`                      | left                   |
| 4     | `parse_and`        | `&&`                             | left                   |
| 5     | `parse_equality`   | `==` `!=`                        | left                   |
| 6     | `parse_comparison` | `<` `<=` `>` `>=`                | left                   |
| 7     | `parse_term`       | `+` `-` `\|` `^`                 | left                   |
| 8     | `parse_factor`     | `*` `/` `%` `<<` `>>` `&`        | left                   |
| 9     | `parse_power`      | `**`                             | right                  |
| 10    | `parse_unary`      | prefix `-` `!` `~` `<-`          | right (prefix)         |
| 11    | `parse_postfix`    | call `()` index `[]` `.` `?.` postfix `?` | left          |
| 12    | `parse_primary`    | literals, names, grouping, ...   |                        |

```ebnf
expr          = assign_expr ;
assign_expr   = range_expr [ assign_op assign_expr ] ;
               /* the left side must be an identifier, a "." access or an index */
range_expr    = or_expr [ ".." or_expr ] ;
or_expr       = and_expr { ( "||" | "??" ) and_expr } ;
and_expr      = equality { "&&" equality } ;
equality      = comparison { ( "==" | "!=" ) comparison } ;
comparison    = term { ( "<" | "<=" | ">" | ">=" ) term } ;
term          = factor { ( "+" | "-" | "|" | "^" ) factor } ;
               /* "-" continues the term only on the same line as its left
                  operand; "|" is not an operator inside a match pattern */
factor        = power { ( "*" | "/" | "%" | "<<" | ">>" | "&" ) power } ;
power         = unary [ "**" power ] ;
unary         = ( "-" | "!" | "~" | "<-" ) unary | postfix ;

postfix       = primary { call | index | member | opt_member | try } ;
call          = "(" args ")" ;         /* "(" on the same line as the callee */
index         = "[" expr "]" ;         /* "[" on the same line as the target */
member        = "." field_name ;
opt_member    = "?." field_name ;
try           = "?" ;                  /* error propagation */
args          = [ expr { "," expr } [ "," ] ] ;
field_name    = IDENT | keyword ;

primary       = INT | FLOAT | DURATION | STRING | TEMPLATE
              | "true" | "false" | "nil"
              | IDENT
              | "(" expr ")"
              | list_lit
              | map_lit
              | fn_lit
              | arrow_fn
              | new_expr
              | chan_expr
              | fg_expr
              | match_expr
              | dyn_import ;

list_lit      = "[" [ expr { "," expr } [ "," ] ] "]" ;
map_lit       = "{" [ map_entry { "," map_entry } [ "," ] ] "}" ;
map_entry     = ( IDENT | STRING | keyword ) ":" expr ;

fn_lit        = "fn" [ IDENT ] fn_tail ;              /* the name is ignored */
arrow_fn      = param_list [ ":" type ] "=>" ( block | expr ) ;
               /* a "{" after "=>" always opens a block */

new_expr      = "new" IDENT { "." IDENT } [ type_args ] [ "(" args ")" ] ;
               /* "new geo.Point()": a struct a module exports (0.8.4) */
               /* "new" is special only when an identifier follows it;
                  "(" must be on the same line */
chan_expr     = "chan" [ "<" /* tokens up to the matching ">" */ ">" ]
                [ "(" [ IDENT ":" ] [ expr ] ")" ] ;
fg_expr       = "fg" postfix ;         /* the postfix must be a call */
match_expr    = "match" expr match_body ;
dyn_import    = "import" "(" expr ")" ;
```

```fogu
let m = {name: "fogu", tags: ["a", "b"], if: 1}
let n = -2 ** 2 + (1 | 2 & 3) << 1
let inc = (x, by = 1) => x + by
let twice = fn (x) { return x * 2 }
let v = m?.meta?.size ?? inc(twice(n))
print(0..n, m.tags[0], m.if, v, fg twice(2), chan<int>(buffer: 4))
```

## 7. Patterns

`match` is both a statement (at statement start) and an expression. Both use
the same body.

```ebnf
match_body    = "{" { match_arm } "}" ;
match_arm     = pattern ( "=>" | "->" ) expr [ ";" | "," ] ;
               /* the arm body is an expression, never a block:
                  "{" there starts a map literal */

pattern       = pattern_atom { "|" pattern_atom } ;            /* alternatives */
pattern_atom  = "_"                                            /* wildcard */
              | IDENT "if" expr                                /* binding + guard */
              | IDENT   /* binding; only when "=>", "->" or "|" follows */
              | equality ;                                     /* value pattern */
               /* a value pattern is any expression of the "equality" level
                  (literals, negative numbers, names in larger expressions,
                  calls ...), parsed with "|" disabled; the subject is compared
                  with its value. There are no range, list, struct or enum
                  patterns. */
```

```fogu
fn kind(x) {
  return match x {
    0 | 1 => "small"
    -1 -> "minus one"
    n if n > 100 => `big ${n}`
    "s" => "string", nil => "nil"; other => `other ${other}`
  }
}
match kind(5) { "small" => print("s") _ => print("?") }
```

## Notes

Rules the parser enforces outside the context-free grammar. Each entry names
the function in `parser.rs` (or `lexer.rs`) that implements it.

1. **No statement terminator.** Line breaks are not tokens. Every simple
   statement ends with an optional `;` (`consume_semicolon`), and stray `;`
   between statements are skipped in programs and blocks (`skip_semicolons`,
   called from `parse_program` and `parse_block`). Two statements may share a
   line with nothing between them (`let a = 1 let b = 2`). Bodies of `select`
   cases do not skip stray `;` (`parse_stmt_list_until_case`), and a `;`
   after `embed Name` in a struct is an error (`parse_struct_body`).
2. **Line breaks that do matter** (`nl_before` compares the line of the
   current token with the previous one). `(` or `[` at the start of a line
   does not continue the previous expression as a call or index
   (`parse_postfix`). A binary `-` at the start of a line is not a
   subtraction: it starts a new statement or match arm, while `+` and all
   other binary operators do continue across lines (`parse_term`). `<-` at
   the start of a line is not a send: it starts a receive expression
   (`parse_stmt`). `return` followed by a line break returns no value
   (`parse_return`). `new Name` followed by `(` on the next line takes no
   arguments (`parse_primary`).
3. **`{` at statement start is a block**, never a map literal (`parse_stmt`).
   In expression position `{` is always a map literal (`parse_primary`).
   There is no struct-literal syntax, so `if`/`while`/`for`/`match` headers
   need no special rule: the header expression simply stops before the `{`
   of the body, because `{` is not a postfix or infix token. A header that
   ends in a map literal still works (`if m == {a: 1} { ... }`).
4. **Arrow function body.** After `=>`, `{` always opens a block
   (`parse_arrow_fn`); to return a map literal, wrap it in parentheses.
5. **Arrow function detection** (`arrow_ahead`). A `(` starts an arrow
   function only if the next token is `)`, an identifier or `...`, and the
   matching `)` is followed by `=>`, or by `:` and then only type-like tokens
   (identifiers, `nil`, `fn`, `chan`, `<`, `>`, `>>`, `|`, `?`, `(`, `)`, `,`)
   up to `=>`. Otherwise `(` is a parenthesised expression. A bare
   `x => x` without parentheses is not accepted.
6. **`fn` at statement start** is a declaration when an identifier follows,
   otherwise an anonymous function expression (`parse_stmt`). In expression
   position `fn name(...) {...}` is accepted and the name is dropped
   (`parse_primary`).
7. **`import` at statement start** is a declaration unless `(` follows; then
   it is the dynamic `import(expr)` expression (`parse_stmt`,
   `parse_primary`).
8. **Contextual words.** `loop` is a statement keyword only at statement
   start directly before `{` (`parse_stmt`, `parse_loop`); `new` is an
   operator only directly before an identifier (`parse_primary`); `any` is
   special only as a type name (`parse_type_atom`). Elsewhere they are
   ordinary identifiers.
9. **Assignment targets.** The left side of `=` and of the compound operators
   is parsed as a `range_expr` and must turn out to be an identifier, a `.`
   access or an index; `?.` chains and everything else are rejected
   (`parse_assignment`).
10. **`fg` needs a call.** The operand of `fg` is parsed as a postfix
    expression and must be a call (`parse_primary`).
11. **Optional chaining scope.** A `?.` makes the whole postfix chain it
    belongs to optional: the chain is wrapped once at its end, or just before
    a postfix `?` (`parse_postfix`).
12. **`|` inside patterns.** While a value pattern is parsed the `no_pipe`
    flag disables `|` as bit-or, so it separates alternatives
    (`parse_pattern_atom`, `parse_term`). Guards and arm bodies are parsed
    with `|` enabled again.
13. **Binding versus value pattern.** A single identifier is a binding only
    when directly followed by `=>`, `->`, `|` or `if`; in any other position
    it is evaluated as an expression (`parse_pattern_atom`).
14. **`>>` in type arguments.** The lexer produces `>>` (and `>>=`) as one
    token; when a type argument list is being closed the parser rewrites it to
    `>` (or `>=`) and consumes one half (`close_angle`). `chan<...>` in
    expression position instead skips tokens while counting `<`, `>` and `>>`
    (`parse_primary`).
15. **Desugaring of parameters** (`parse_param_list`, `parse_fn_tail`,
    `with_prologue`). A default `p = e` becomes the statement `p = p ?? e` at
    the start of the body; an annotation `p: T` becomes a run-time type check
    there (omitted for `any` and type parameters); a variadic `...p` is stored
    under the name `...p`, must be last, cannot have a default, and its
    annotation checks only that it is a `list`. A return type becomes the
    first statement of the body.
16. **Types are mostly erased** (`parse_type_atom`, `parse_type_params`,
    `parse_struct_body`). Function types keep only "is a function"; type
    parameters and their bounds, type arguments of `new` and `chan`, and
    struct field types are parsed and discarded.
17. **Other desugarings.** `loop { }` becomes `while true { }` (`parse_loop`);
    `(params) => expr` becomes a function whose body is `return expr`
    (`parse_arrow_fn`); `return a, b` builds a tuple (`parse_return`); a
    method named `new` in a struct becomes its constructor
    (`parse_struct_body`); a unary minus applied to a numeric literal is
    folded into the literal (`parse_unary`).
18. **Largest negative integer.** `9223372036854775808` is accepted only
    directly after a unary `-` (`Lexer::read_number`, `parse_unary`,
    `parse_primary`).
19. **Duration suffix.** A number is a duration only if the complete word that
    follows it is a unit; otherwise the number ends and an identifier begins
    (`5kb` is `5` followed by the name `kb`) (`Lexer::duration_unit`).
20. **Exports.** The same name may not be exported twice in a module
    (`note_export`); `import { default }` must rename it and
    `export { default }` is only valid with `from` (`parse_name_list_at`,
    `parse_export`).
21. **Template interpolation.** Each `${...}` is tokenized separately and must
    contain exactly one expression (`Lexer::read_template`,
    `Lexer::read_interpolation`, `parse_primary`).

### Not part of the grammar

Checked against the binary and rejected: `enum`, type aliases (`type T = ...`),
`defer`, C-style `for`, `let` without an initializer, `if` as an expression,
struct literals (`P { x: 1 }`; use `new P(...)`), slice syntax (`a[1:2]`;
`a[0..2]` parses as an index by a range value), character literals (`'a'` is a
string), size literals (`5kb`), float literals without a digit on both sides
of the dot (`1.`, `.5`), range patterns in `match`, block bodies in `match`
arms, commas between struct members, a trailing comma in `let [a, b,]`,
chained ranges (`a..b..c`), and arrow functions without parentheses.
