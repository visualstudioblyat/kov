# kov language specification

version 0.1.0

## grammar

```ebnf
program     = top_item* ;
top_item    = import | board | function | interrupt | struct_def
            | enum_def | const_def | static_def | type_alias
            | extern_fn | trait_def | impl_block | const_assert ;

import      = "import" path ";" ;
path        = ident ( "::" ident )* ;

board       = "board" ident "{" board_field ("," board_field)* ","? "}" ;
board_field = ident ":" ident ("@" expr)? ;

function    = attribute* "fn" ident type_params? "(" param_list ")" type? block ;
type_params = "<" type_param ("," type_param)* ">" ;
type_param  = ident (":" ident ("+" ident)*)? ;
param_list  = (param ("," param)*)? ;
param       = ident ":" type ;

interrupt   = "interrupt" "(" ident "," "priority" "=" int ")" "fn" ident "(" ")" block ;

struct_def  = "struct" ident type_params? "{" (ident ":" type ","?)* "}" ;
enum_def    = "enum" ident "{" (ident ("(" type_list ")")? ","?)* "}" ;
trait_def   = "trait" ident "{" trait_method* "}" ;
trait_method= "fn" ident "(" param_list ")" type? (";" | block) ;
impl_block  = "impl" ident ("for" ident)? "{" function* "}" ;
extern_fn   = "extern" string "fn" ident "(" param_list ")" type? ";" ;
const_assert= "static_assert" "(" expr ")" ";" ;
const_def   = "const" ident ":" type "=" expr ";" ;
static_def  = "static" "mut"? ident ":" type "=" expr ";" ;
type_alias  = "type" ident "=" type ";" ;

block       = "{" stmt* "}" ;
stmt        = let_stmt | assign_stmt | expr_stmt | return_stmt
            | if_stmt | loop_stmt | while_stmt | for_stmt | match_stmt
            | break_stmt | continue_stmt | asm_stmt ;

let_stmt    = "let" "mut"? ident (":" type)? "=" expr ";" ;
assign_stmt = expr assign_op expr ";" ;
expr_stmt   = expr ";" ;
return_stmt = "return" expr? ";" ;
if_stmt     = "if" expr block ("else" (block | if_stmt))? ;
loop_stmt   = label? "loop" block ;
while_stmt  = label? "while" expr block ;
for_stmt    = label? "for" ident "in" expr ".." expr block ;
match_stmt  = "match" expr "{" match_arm* "}" ;
match_arm   = pattern "=>" expr ","? ;
break_stmt  = "break" lifetime? ";" ;
continue_stmt = "continue" lifetime? ";" ;
asm_stmt    = "asm" "!" "(" string ("," asm_operand)* ")" ";" ;
asm_operand = ident "(" ident ")" expr ;

label       = lifetime ":" ;
lifetime    = "'" ident ;

pattern     = int | "_" | ident | ident "(" ident_list? ")" ;

type        = primitive | ident type_args? | "&" "mut"? type | "*" type
            | "[" type ";" expr "]" | "!" type | "fn" "(" type_list ")" type? ;
type_args   = "<" type ("," type)* ">" ;
primitive   = "u8" | "u16" | "u32" | "u64" | "i8" | "i16" | "i32" | "i64"
            | "bool" | "void" | "usize" | "isize" ;

expr        = literal | ident | expr binop expr | unaryop expr
            | expr "." ident | expr "." ident "(" expr_list ")"
            | expr "(" expr_list ")" | expr "[" expr "]"
            | "try" expr | "[" expr_list "]"
            | ident "{" (ident ":" expr ","?)* "}"
            | "." ident ;

binop       = "+" | "-" | "*" | "/" | "%" | "&" | "|" | "^"
            | "<<" | ">>" | "==" | "!=" | "<" | ">" | "<=" | ">="
            | "&&" | "||" | "%+" | "%-" | "%*" ;
unaryop     = "-" | "!" | "~" | "&" | "&mut" | "*" ;
assign_op   = "=" | "+=" | "-=" | "*=" | "/=" | "%=" | "&=" | "|=" | "^=" ;

attribute   = "#" "[" ident ("(" expr_list ")")? "]" ;
```

## types

| type | size | description |
|------|------|-------------|
| u8 | 1 | unsigned 8-bit |
| u16 | 2 | unsigned 16-bit |
| u32 | 4 | unsigned 32-bit |
| u64 | 8 | unsigned 64-bit |
| i8 | 1 | signed 8-bit |
| i16 | 2 | signed 16-bit |
| i32 | 4 | signed 32-bit |
| i64 | 8 | signed 64-bit |
| bool | 1 | true or false |
| void | 0 | no value |
| !T | 8 | error union (payload + tag) |

no implicit integer promotion. u8 + u32 is a compile error.

## memory model

- globals in .data (initialized) or .bss (zeroed)
- locals in registers, spill to stack when exhausted
- structs on stack with natural alignment
- arrays on stack, fixed size
- strings in global data, accessed by pointer

## calling convention

RISC-V psABI: a0-a7 arguments, a0 return, s0-s11 callee-saved.
16-byte aligned frames. callee saves only registers it uses.

## error handling

functions returning !T put payload in a0, error tag in a1.
try checks a1, propagates if nonzero, unwraps a0 if zero.

## safety guarantees

- peripheral ownership: double-claim is a compile error
- #[stack(N)]: compilation fails if worst-case stack > N bytes
- #[max_cycles(N)]: compilation fails if WCET > N cycles
- interrupt safety: shared globals between main/ISR flagged
- DMA safety: buffer access during active transfer is an error
- no implicit integer promotion
- match exhaustiveness required
