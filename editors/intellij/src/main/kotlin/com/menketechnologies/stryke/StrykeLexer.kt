package com.menketechnologies.stryke

import com.intellij.lexer.LexerBase
import com.intellij.psi.TokenType
import com.intellij.psi.tree.IElementType

/**
 * Hand-rolled lexer for stryke. Recognizes:
 *
 *   * `#` line comments
 *   * single, double, and backtick strings (with `\` escapes)
 *   * heredocs (basic `<<EOT ... EOT`)
 *   * integer and float numbers (with `_` separators and `e` exponents)
 *   * sigil variables `$`, `@`, `%` — split into scalar / array / hash /
 *     special-var / topic / block-param
 *   * package paths `Foo::Bar::Baz` (separator highlighted distinctly)
 *   * pipes `|>`, `|>>`, `~>`
 *   * arrows `->`, `=>` (fat comma)
 *   * regex literals `/.../flags`
 *   * keyword categories: declaration / fn-decl / control / phase / word-op /
 *     boolean / undef
 *   * paren / brace / bracket / comma / semicolon as separate categories so the
 *     user can color them independently
 *
 * The LSP overlays server-side semantic tokens on top of this; the lexer
 * provides instant feedback before the LSP processes the document.
 */
class StrykeLexer : LexerBase() {
    private var buf: CharSequence = ""
    private var endOffset = 0
    private var pos = 0
    private var tokenStart = 0
    private var tokenEnd = 0
    private var tokenType: IElementType? = null
    private var state = 0
    private var pendingFormatLen = 0
    private var interpBraceDepth = 0
    private var arrayInterpBracketDepth = 0
    /// Set true after emitting a `fn` / `sub` / `method` / `class` /
    /// `struct` / `trait` / `enum` keyword so the NEXT identifier
    /// gets colored as a declaration name (`FUNCTION_DECL`) rather
    /// than a plain identifier or call. Cleared on the next emission.
    /// Lossy on incremental relexing — IntelliJ may start re-lexing
    /// mid-file with this flag reset, but the worst case is a flicker
    /// of decl-name coloring; acceptable for a syntax-only highlight.
    private var lastWasFnIntro = false

    override fun start(buffer: CharSequence, startOffset: Int, endOffset: Int, initialState: Int) {
        buf = buffer
        this.endOffset = endOffset
        pos = startOffset
        state = initialState
        advance()
    }

    override fun getState(): Int = state
    override fun getTokenType(): IElementType? = tokenType
    override fun getTokenStart(): Int = tokenStart
    override fun getTokenEnd(): Int = tokenEnd
    override fun getBufferSequence(): CharSequence = buf
    override fun getBufferEnd(): Int = endOffset

    override fun advance() {
        tokenStart = pos
        if (pos >= endOffset) {
            tokenType = null
            tokenEnd = pos
            return
        }
        // Resume continuation of a `"..."` interpolation `#{EXPR}` block —
        // the previous advance() chunk emitted the literal-prefix String,
        // and now we either emit the `#{` opener (STATE_IN_DQ_INTERP_START)
        // or walk the interior token-by-token until `}` (STATE_IN_DQ_INTERP),
        // then return to string mode (STATE_IN_DQ_STRING).
        if (state == STATE_IN_DQ_INTERP || state == STATE_IN_DQ_INTERP_START) {
            lexInsideInterpolation()
            return
        }
        // Perl-style array-ref interpolation `@{[ EXPR ]}` — same idea
        // as `#{EXPR}` but the closer is `]}` instead of `}`. Stryke
        // accepts both spellings; the IDE must color the interior as
        // code rather than as literal string text.
        if (state == STATE_IN_DQ_ARRAY_INTERP || state == STATE_IN_DQ_ARRAY_INTERP_START) {
            lexInsideArrayInterpolation()
            return
        }
        // Resume scanning the suffix of an interrupted `"..."` (after an
        // interpolation closes). Picks up exactly where consumeString left
        // off — no missing `#`-as-comment misinterpretation.
        if (state == STATE_IN_DQ_STRING) {
            consumeDoubleStringContinuation()
            return
        }
        // We're at a `$`/`@`/`%` interpolation marker inside a `"..."`.
        // Emit the sigil-var as one token (SCALAR_VAR / ARRAY_VAR / HASH_VAR)
        // then return to string mode so the suffix is consumed as STRING.
        if (state == STATE_IN_DQ_SIGIL_VAR) {
            val sigil = buf[pos]
            consumeSigilVar(sigil)
            state = STATE_IN_DQ_STRING
            return
        }
        // Regex flags — the letter run immediately after `/pattern/`.
        if (state == STATE_REGEX_FLAGS_PENDING) {
            var p = pos
            while (p < endOffset && buf[p].isLetter()) p++
            if (p > pos) {
                tokenStart = pos
                tokenEnd = p
                pos = p
                tokenType = StrykeTokenTypes.REGEX_FLAGS
                state = STATE_NORMAL
                return
            }
            // No flags after all — fall through to normal lexing.
            state = STATE_NORMAL
        }
        // printf format spec — emit `%d` / `%10.2f` / `%-15s` etc. as
        // one STRING_FORMAT token, then return to string mode.
        if (state == STATE_IN_DQ_FORMAT) {
            tokenStart = pos
            tokenEnd = pos + pendingFormatLen
            pos = tokenEnd
            tokenType = StrykeTokenTypes.STRING_FORMAT
            pendingFormatLen = 0
            state = STATE_IN_DQ_STRING
            return
        }
        val c = buf[pos]
        when {
            c == '#' && isCommentStart() -> consumeLineComment()
            c == '\n' || c == '\r' || c == ' ' || c == '\t' -> consumeWhitespace()
            c == '"' -> consumeDoubleQuoteString()
            c == '\'' -> consumeString('\'')
            c == '`' -> consumeString('`')
            // Heredoc: <<EOT, <<'EOT', <<"EOT", <<~EOT
            c == '<' && peek(1) == '<' && isHeredocStart(2) -> consumeHeredoc()
            c == '-' && peek(1) == '>' -> emit(2, StrykeTokenTypes.ARROW_OP)
            c == '=' && peek(1) == '>' -> emit(2, StrykeTokenTypes.FAT_COMMA)
            // PipeForward `|>`. The Rust lexer doesn't have `|>>` —
            // a `|>>` source byte sequence tokenizes as `|>` then `>`.
            c == '|' && peek(1) == '>' -> emit(2, StrykeTokenTypes.PIPE)
            // Thread-arrows (`~>` family). Order matters: try 3-char
            // forms first (`~s>>` / `~p>>` / `~d>>`), then 2-char
            // `~s>` / `~p>` / `~d>`, then `~>>`, then bare `~>`.
            c == '~' && peek(1) == 's' && peek(2) == '>' && peek(3) == '>' ->
                emit(4, StrykeTokenTypes.PIPE)
            c == '~' && peek(1) == 'p' && peek(2) == '>' && peek(3) == '>' ->
                emit(4, StrykeTokenTypes.PIPE)
            c == '~' && peek(1) == 'd' && peek(2) == '>' && peek(3) == '>' ->
                emit(4, StrykeTokenTypes.PIPE)
            c == '~' && peek(1) == 's' && peek(2) == '>' -> emit(3, StrykeTokenTypes.PIPE)
            c == '~' && peek(1) == 'p' && peek(2) == '>' -> emit(3, StrykeTokenTypes.PIPE)
            c == '~' && peek(1) == 'd' && peek(2) == '>' -> emit(3, StrykeTokenTypes.PIPE)
            c == '~' && peek(1) == '>' && peek(2) == '>' -> emit(3, StrykeTokenTypes.PIPE)
            c == '~' && peek(1) == '>' -> emit(2, StrykeTokenTypes.PIPE)
            c == '=' && peek(1) == '~' -> emit(2, StrykeTokenTypes.REGEX_BIND)
            c == '!' && peek(1) == '~' -> emit(2, StrykeTokenTypes.REGEX_BIND)
            c == '.' && peek(1) == '.' -> emit(2, StrykeTokenTypes.RANGE)
            c == '$' || c == '@' || c == '%' -> consumeSigilVar(c)
            c.isDigit() -> consumeNumber()
            c == '_' || c.isLetter() -> consumeWord()
            c == '/' -> consumeRegexOrSlash()
            isCompoundAssign(c) -> emit(2, StrykeTokenTypes.ASSIGN_OP)
            c == '=' && peek(1) != '=' -> emit(1, StrykeTokenTypes.ASSIGN_OP)
            c == '(' -> emit(1, StrykeTokenTypes.LPAREN)
            c == ')' -> emit(1, StrykeTokenTypes.RPAREN)
            c == '{' -> emit(1, StrykeTokenTypes.LBRACE)
            c == '}' -> emit(1, StrykeTokenTypes.RBRACE)
            c == '[' -> emit(1, StrykeTokenTypes.LBRACKET)
            c == ']' -> emit(1, StrykeTokenTypes.RBRACKET)
            c == ',' -> emit(1, StrykeTokenTypes.COMMA)
            c == ';' -> emit(1, StrykeTokenTypes.SEMICOLON)
            c == '.' -> emit(1, StrykeTokenTypes.DOT)
            // `::` between package segments — own color category so the
            // user can pick a separate color for separators vs name
            // segments.
            c == ':' && peek(1) == ':' -> emit(2, StrykeTokenTypes.PACKAGE_SEPARATOR)
            isOperatorChar(c) -> emit(1, StrykeTokenTypes.OPERATOR)
            else -> emit(1, TokenType.BAD_CHARACTER)
        }
    }

    private fun peek(off: Int): Char = if (pos + off < endOffset) buf[pos + off] else ' '

    private fun emit(len: Int, tt: IElementType) {
        tokenEnd = (pos + len).coerceAtMost(endOffset)
        pos = tokenEnd
        tokenType = tt
    }

    private fun isCompoundAssign(c: Char): Boolean {
        if (peek(1) != '=') return false
        // Avoid matching ==, !=, <=, >=, =~ (regex_bind handled above)
        if (c == '=' || c == '!' || c == '<' || c == '>') return false
        return c in "+-*/%.|&^"
    }

    private fun consumeLineComment() {
        var p = pos
        while (p < endOffset && buf[p] != '\n') p++
        tokenEnd = p
        pos = p
        // Heuristic: `##` or `#:` is a doc-style comment
        val len = p - tokenStart
        tokenType = if (len >= 2 && (buf[tokenStart + 1] == '#' || buf[tokenStart + 1] == '!')) {
            StrykeTokenTypes.DOC_COMMENT
        } else {
            StrykeTokenTypes.COMMENT
        }
    }

    /**
     * `#` is a real comment opener UNLESS it's part of a parameter
     * expansion:
     *   `$#var`   — last-index of `@var`
     *   `$#`      — special: last arg-index of current sub
     *   `${#var}` — string length of `$var`
     *
     * Without this guard the lexer paints everything after `$#xs` as
     * comment, including the matching `]]` and the next stage of a
     * ternary. Same bug class as the zshrs LSP linter fix.
     */
    private fun isCommentStart(): Boolean {
        if (buf[pos] != '#') return false
        if (pos > 0 && buf[pos - 1] == '$') return false             // $# / $#var
        if (pos >= 2 && buf[pos - 1] == '{' && buf[pos - 2] == '$') return false  // ${#var}
        return true
    }

    private fun consumeWhitespace() {
        var p = pos
        while (p < endOffset && (buf[p] == ' ' || buf[p] == '\t' || buf[p] == '\n' || buf[p] == '\r')) p++
        tokenEnd = p
        pos = p
        tokenType = TokenType.WHITE_SPACE
    }

    private fun consumeString(quote: Char) {
        var p = pos + 1
        while (p < endOffset) {
            val c = buf[p]
            if (c == '\\' && p + 1 < endOffset) { p += 2; continue }
            if (c == quote) { p++; break }
            if (c == '\n' && quote != '"') break
            p++
        }
        tokenEnd = p
        pos = p
        tokenType = StrykeTokenTypes.STRING
    }

    /**
     * Double-quoted string with stryke-style `#{EXPR}` interpolation. Emits
     * one STRING token for the literal run from the opening `"` up to (but
     * not including) any `#{`, then on the next `advance()` enters interp
     * mode and emits real tokens for the expression inside the braces, then
     * resumes string mode for the next literal run.
     *
     * Critically: `#` outside `#{` is NEVER a comment opener inside a
     * string. This is the fix for the JetBrains plugin bug where naive
     * `#` handling colored interpolations like comments.
     */
    private fun consumeDoubleQuoteString() {
        // pos is at the opening `"`. Scan forward until we hit the closing
        // `"`, a `#{EXPR}` interpolation, or a sigil-var interpolation
        // (`$var`, `@arr`, `%h`, `${name}`, `$1`, etc.).
        var p = pos + 1
        while (p < endOffset) {
            val c = buf[p]
            if (c == '\\' && p + 1 < endOffset) { p += 2; continue }
            if (c == '"') { p++; break }
            if (c == '#' && p + 1 < endOffset && buf[p + 1] == '{') {
                // Emit the literal run up to (NOT including) `#{`. Mark the
                // lexer as in-string so on the next advance() we step into
                // interpolation mode.
                tokenEnd = p
                pos = p
                tokenType = StrykeTokenTypes.STRING
                state = STATE_IN_DQ_INTERP_START
                return
            }
            // Perl-style `@{[ EXPR ]}` array-ref interpolation — checked
            // BEFORE the `@`-sigil-var branch below so the `@{[` form
            // doesn't get mis-classified as a `@{name}` array variable.
            if (c == '@' && p + 2 < endOffset && buf[p + 1] == '{' && buf[p + 2] == '[') {
                tokenEnd = p
                pos = p
                tokenType = StrykeTokenTypes.STRING
                state = STATE_IN_DQ_ARRAY_INTERP_START
                return
            }
            if ((c == '$' || c == '@' || c == '%')
                && p + 1 < endOffset
                && isInStringSigilVarStart(buf[p + 1])
            ) {
                // Emit the literal prefix as STRING; next advance() emits
                // the sigil-var via consumeSigilVar, then the continuation
                // resumes scanning the rest of the string.
                tokenEnd = p
                pos = p
                tokenType = StrykeTokenTypes.STRING
                state = STATE_IN_DQ_SIGIL_VAR
                return
            }
            // printf format specifier: `%d`, `%s`, `%10.2f`, `%-15s`, etc.
            // Highlight distinctly so the user can see `%s` isn't a hash.
            if (c == '%') {
                val formatLen = printfFormatLen(p)
                if (formatLen > 0) {
                    // Emit literal prefix first; continuation handles the
                    // format spec on the next advance().
                    tokenEnd = p
                    pos = p
                    tokenType = StrykeTokenTypes.STRING
                    state = STATE_IN_DQ_FORMAT
                    pendingFormatLen = formatLen
                    return
                }
            }
            p++
        }
        tokenEnd = p
        pos = p
        state = STATE_NORMAL
        tokenType = StrykeTokenTypes.STRING
    }

    /**
     * Continuation of a double-quoted string after an interpolation `}`
     * (or after a sigil-var) closes. Behaves like [consumeDoubleQuoteString]
     * but doesn't skip an opening `"` (there isn't one — we're mid-string).
     */
    private fun consumeDoubleStringContinuation() {
        var p = pos
        while (p < endOffset) {
            val c = buf[p]
            if (c == '\\' && p + 1 < endOffset) { p += 2; continue }
            if (c == '"') { p++; break }
            if (c == '#' && p + 1 < endOffset && buf[p + 1] == '{') {
                tokenEnd = p
                pos = p
                tokenType = StrykeTokenTypes.STRING
                state = STATE_IN_DQ_INTERP_START
                return
            }
            if (c == '@' && p + 2 < endOffset && buf[p + 1] == '{' && buf[p + 2] == '[') {
                tokenEnd = p
                pos = p
                tokenType = StrykeTokenTypes.STRING
                state = STATE_IN_DQ_ARRAY_INTERP_START
                return
            }
            if ((c == '$' || c == '@' || c == '%')
                && p + 1 < endOffset
                && isInStringSigilVarStart(buf[p + 1])
            ) {
                tokenEnd = p
                pos = p
                tokenType = StrykeTokenTypes.STRING
                state = STATE_IN_DQ_SIGIL_VAR
                return
            }
            if (c == '%') {
                val formatLen = printfFormatLen(p)
                if (formatLen > 0) {
                    tokenEnd = p
                    pos = p
                    tokenType = StrykeTokenTypes.STRING
                    state = STATE_IN_DQ_FORMAT
                    pendingFormatLen = formatLen
                    return
                }
            }
            p++
        }
        tokenEnd = p
        pos = p
        state = STATE_NORMAL
        tokenType = StrykeTokenTypes.STRING
    }

    /**
     * Return the length (in chars) of a printf-style format specifier
     * starting at `start` (which must point at `%`), or 0 if the chars
     * after `%` don't form a valid format spec. Recognized grammar:
     *   `%` [flags `-+0 #`]* [width digits]? (`.` [precision digits]+)?
     *       [length `l`/`h`/`L`/`q`/`z`/`j`/`t`]?
     *       conversion char `diouxXeEfgGsScCpn%`
     * Permissive on stryke specifics — covers Perl's printf/sprintf
     * conversions plus the doubled `%%` for a literal percent.
     */
    private fun printfFormatLen(start: Int): Int {
        if (start >= endOffset || buf[start] != '%') return 0
        var p = start + 1
        if (p >= endOffset) return 0
        // Doubled `%%` — literal percent. Highlight as one format spec.
        if (buf[p] == '%') return 2
        // flags
        while (p < endOffset && buf[p] in FORMAT_FLAGS) p++
        // width
        while (p < endOffset && buf[p].isDigit()) p++
        // precision
        if (p < endOffset && buf[p] == '.') {
            p++
            while (p < endOffset && buf[p].isDigit()) p++
        }
        // length modifier
        while (p < endOffset && buf[p] in FORMAT_LENGTH) p++
        // conversion
        if (p < endOffset && buf[p] in FORMAT_CONV) {
            return (p - start) + 1
        }
        // No conversion char found → not a format spec.
        return 0
    }

    /**
     * True if the char immediately after a `$`/`@`/`%` would start a
     * real var interpolation (rather than a literal sigil character in
     * the string). Conservative on purpose — matches the LSP
     * semantic-tokens rule: only letter / `_` / `{` qualify. Digit
     * tails (`$1`, `%8s`, `@10`) are NOT treated as variables to
     * avoid false-positive coloring of printf format specifiers like
     * `"%-15s %8s %10s\n"` — these are string content, not refs.
     */
    private fun isInStringSigilVarStart(c: Char): Boolean {
        return c == '_' || c.isLetter() || c == '{'
    }

    /**
     * One advance() inside the `#{EXPR}` interpolation block. Tracks a
     * brace-nesting counter so braces inside the expression don't close
     * the interpolation prematurely. On the closing `}` we flip back to
     * STATE_IN_DQ_STRING so the next advance() resumes the string suffix.
     */
    private fun lexInsideInterpolation() {
        // First call after entering interp mode: emit the `#{` opener.
        if (state == STATE_IN_DQ_INTERP_START) {
            tokenStart = pos
            tokenEnd = pos + 2
            pos = tokenEnd
            tokenType = StrykeTokenTypes.OPERATOR
            state = STATE_IN_DQ_INTERP
            interpBraceDepth = 1
            return
        }
        // Closing `}` returns us to string mode.
        if (pos < endOffset && buf[pos] == '}' && interpBraceDepth == 1) {
            tokenStart = pos
            tokenEnd = pos + 1
            pos = tokenEnd
            tokenType = StrykeTokenTypes.OPERATOR
            state = STATE_IN_DQ_STRING
            interpBraceDepth = 0
            return
        }
        // Otherwise lex one normal token (recursing into `advance` with a
        // borrowed state so the normal dispatch runs); maintain brace depth
        // so nested `{}` inside the expression don't end interp early.
        val savedState = state
        state = STATE_NORMAL
        val saveStart = tokenStart
        // Run the normal dispatcher exactly once.
        runOneNormalAdvance()
        // Track brace nesting on simple `{` / `}` tokens.
        when (tokenType) {
            StrykeTokenTypes.LBRACE -> interpBraceDepth++
            StrykeTokenTypes.RBRACE -> interpBraceDepth--
            else -> {}
        }
        // Stay in interp mode for the next advance().
        state = if (interpBraceDepth > 0) STATE_IN_DQ_INTERP else STATE_IN_DQ_STRING
        tokenStart = saveStart // preserve start offset for the token emitted
        // savedState was the entry state we no longer need.
        @Suppress("UNUSED_VARIABLE") val _unused = savedState
    }

    /**
     * One advance() inside the Perl-style `@{[ EXPR ]}` interpolation
     * block. Symmetric to [lexInsideInterpolation] but the closer is
     * the 2-char sequence `]}` instead of a bare `}`. Tracks `[`/`]`
     * depth so nested array literals inside the expression don't end
     * the interp prematurely. On the closing `]}` we emit it as one
     * OPERATOR token, then flip back to STATE_IN_DQ_STRING.
     */
    private fun lexInsideArrayInterpolation() {
        // First call: emit the `@{[` opener (3 chars).
        if (state == STATE_IN_DQ_ARRAY_INTERP_START) {
            tokenStart = pos
            tokenEnd = pos + 3
            pos = tokenEnd
            tokenType = StrykeTokenTypes.OPERATOR
            state = STATE_IN_DQ_ARRAY_INTERP
            arrayInterpBracketDepth = 1
            return
        }
        // Closing `]}` at outer depth — emit as one OPERATOR and return
        // to string mode.
        if (pos + 1 < endOffset
            && buf[pos] == ']'
            && buf[pos + 1] == '}'
            && arrayInterpBracketDepth == 1
        ) {
            tokenStart = pos
            tokenEnd = pos + 2
            pos = tokenEnd
            tokenType = StrykeTokenTypes.OPERATOR
            state = STATE_IN_DQ_STRING
            arrayInterpBracketDepth = 0
            return
        }
        // Otherwise lex one normal token. Maintain bracket depth so
        // nested `[]` inside the expression don't end interp early.
        state = STATE_NORMAL
        val saveStart = tokenStart
        runOneNormalAdvance()
        when (tokenType) {
            StrykeTokenTypes.LBRACKET -> arrayInterpBracketDepth++
            StrykeTokenTypes.RBRACKET -> arrayInterpBracketDepth--
            else -> {}
        }
        state = if (arrayInterpBracketDepth > 0) STATE_IN_DQ_ARRAY_INTERP else STATE_IN_DQ_STRING
        tokenStart = saveStart
    }

    /**
     * Run the normal top-level dispatch logic once, used while inside an
     * interpolation block. Mirrors the body of [advance] minus the
     * state-resume checks.
     */
    private fun runOneNormalAdvance() {
        tokenStart = pos
        if (pos >= endOffset) {
            tokenType = null
            tokenEnd = pos
            return
        }
        val c = buf[pos]
        when {
            c == '#' && isCommentStart() -> consumeLineComment()
            c == '\n' || c == '\r' || c == ' ' || c == '\t' -> consumeWhitespace()
            c == '"' -> consumeDoubleQuoteString()
            c == '\'' -> consumeString('\'')
            c == '`' -> consumeString('`')
            c == '<' && peek(1) == '<' && isHeredocStart(2) -> consumeHeredoc()
            c == '-' && peek(1) == '>' -> emit(2, StrykeTokenTypes.ARROW_OP)
            c == '=' && peek(1) == '>' -> emit(2, StrykeTokenTypes.FAT_COMMA)
            // PipeForward `|>`. The Rust lexer doesn't have `|>>` —
            // a `|>>` source byte sequence tokenizes as `|>` then `>`.
            c == '|' && peek(1) == '>' -> emit(2, StrykeTokenTypes.PIPE)
            // Thread-arrows (`~>` family). Order matters: try 3-char
            // forms first (`~s>>` / `~p>>` / `~d>>`), then 2-char
            // `~s>` / `~p>` / `~d>`, then `~>>`, then bare `~>`.
            c == '~' && peek(1) == 's' && peek(2) == '>' && peek(3) == '>' ->
                emit(4, StrykeTokenTypes.PIPE)
            c == '~' && peek(1) == 'p' && peek(2) == '>' && peek(3) == '>' ->
                emit(4, StrykeTokenTypes.PIPE)
            c == '~' && peek(1) == 'd' && peek(2) == '>' && peek(3) == '>' ->
                emit(4, StrykeTokenTypes.PIPE)
            c == '~' && peek(1) == 's' && peek(2) == '>' -> emit(3, StrykeTokenTypes.PIPE)
            c == '~' && peek(1) == 'p' && peek(2) == '>' -> emit(3, StrykeTokenTypes.PIPE)
            c == '~' && peek(1) == 'd' && peek(2) == '>' -> emit(3, StrykeTokenTypes.PIPE)
            c == '~' && peek(1) == '>' && peek(2) == '>' -> emit(3, StrykeTokenTypes.PIPE)
            c == '~' && peek(1) == '>' -> emit(2, StrykeTokenTypes.PIPE)
            c == '=' && peek(1) == '~' -> emit(2, StrykeTokenTypes.REGEX_BIND)
            c == '!' && peek(1) == '~' -> emit(2, StrykeTokenTypes.REGEX_BIND)
            c == '.' && peek(1) == '.' -> emit(2, StrykeTokenTypes.RANGE)
            c == '$' || c == '@' || c == '%' -> consumeSigilVar(c)
            c.isDigit() -> consumeNumber()
            c == '_' || c.isLetter() -> consumeWord()
            c == '/' -> consumeRegexOrSlash()
            isCompoundAssign(c) -> emit(2, StrykeTokenTypes.ASSIGN_OP)
            c == '=' && peek(1) != '=' -> emit(1, StrykeTokenTypes.ASSIGN_OP)
            c == '(' -> emit(1, StrykeTokenTypes.LPAREN)
            c == ')' -> emit(1, StrykeTokenTypes.RPAREN)
            c == '{' -> emit(1, StrykeTokenTypes.LBRACE)
            c == '}' -> emit(1, StrykeTokenTypes.RBRACE)
            c == '[' -> emit(1, StrykeTokenTypes.LBRACKET)
            c == ']' -> emit(1, StrykeTokenTypes.RBRACKET)
            c == ',' -> emit(1, StrykeTokenTypes.COMMA)
            c == ';' -> emit(1, StrykeTokenTypes.SEMICOLON)
            c == '.' -> emit(1, StrykeTokenTypes.DOT)
            // `::` between package segments — own color category so the
            // user can pick a separate color for separators vs name
            // segments.
            c == ':' && peek(1) == ':' -> emit(2, StrykeTokenTypes.PACKAGE_SEPARATOR)
            isOperatorChar(c) -> emit(1, StrykeTokenTypes.OPERATOR)
            else -> emit(1, TokenType.BAD_CHARACTER)
        }
    }

    private fun isHeredocStart(off: Int): Boolean {
        var p = pos + off
        if (p < endOffset && buf[p] == '~') p++
        if (p < endOffset && (buf[p] == '\'' || buf[p] == '"')) p++
        return p < endOffset && (buf[p] == '_' || buf[p].isLetter())
    }

    private fun consumeHeredoc() {
        // Capture the opening line: `<<EOT` (or `<<~"EOT"`).
        var p = pos + 2
        if (p < endOffset && buf[p] == '~') p++
        val quote = if (p < endOffset && (buf[p] == '\'' || buf[p] == '"')) {
            val q = buf[p]; p++; q
        } else 0.toChar()
        val nameStart = p
        while (p < endOffset && (buf[p] == '_' || buf[p].isLetterOrDigit())) p++
        val name = buf.subSequence(nameStart, p).toString()
        if (quote != 0.toChar() && p < endOffset && buf[p] == quote) p++
        // Scan to end of file or to a line equal to `name` (allowing optional leading whitespace for `<<~`).
        while (p < endOffset) {
            // advance past current line
            while (p < endOffset && buf[p] != '\n') p++
            if (p >= endOffset) break
            p++ // consume newline
            // check for terminator at start of next line
            val lineStart = p
            var q = lineStart
            while (q < endOffset && (buf[q] == ' ' || buf[q] == '\t')) q++
            val termEnd = q + name.length
            if (termEnd <= endOffset &&
                buf.subSequence(q, termEnd).toString() == name &&
                (termEnd == endOffset || buf[termEnd] == '\n' || buf[termEnd] == '\r')
            ) {
                p = termEnd
                break
            }
        }
        tokenEnd = p
        pos = p
        tokenType = StrykeTokenTypes.HEREDOC
    }

    private fun consumeSigilVar(sigil: Char) {
        val p0 = pos
        var p = pos + 1
        // Empty sigil (e.g. used as operator)
        if (p >= endOffset) {
            tokenEnd = p; pos = p
            tokenType = StrykeTokenTypes.OPERATOR
            return
        }
        // `$#var` (last index of @var) and bare `$#` (last arg index).
        // Consume `#` plus any trailing identifier-name chars as one
        // SCALAR_VAR token so the `#` doesn't trigger the comment
        // path and the var name doesn't fragment.
        if (sigil == '$' && buf[p] == '#') {
            p++ // consume `#`
            while (p < endOffset && (buf[p] == '_' || buf[p].isLetterOrDigit())) p++
            tokenEnd = p; pos = p
            tokenType = StrykeTokenTypes.SCALAR_VAR
            return
        }
        // ${...} / @{...} / %{...} block-deref — treat the whole thing as a variable.
        if (buf[p] == '{') {
            p++
            while (p < endOffset && buf[p] != '}' && buf[p] != '\n') p++
            if (p < endOffset) p++
            tokenEnd = p; pos = p
            tokenType = varKind(sigil)
            return
        }
        // Punctuation specials — single char tail.
        if (isSpecialVarChar(buf[p])) {
            val first = buf[p]
            val name = "$sigil$first"
            p++
            // Caret specials like `$^X`, `@^CAPTURE`, `%^HOOK`,
            // `$^WARNING_BITS` — a single `^` carries an uppercase
            // identifier tail. Without this the lexer stopped at
            // `%^` and colored the trailing `HOOK` as a bareword.
            if (first == '^') {
                while (p < endOffset && (buf[p] == '_' || buf[p].isLetterOrDigit())) p++
            }
            tokenEnd = p; pos = p
            tokenType = when {
                name == "\$_" || name == "@_" -> StrykeTokenTypes.TOPIC_VAR
                else -> StrykeTokenTypes.SPECIAL_VAR
            }
            return
        }
        // Underscore positional block params with outer-chain ascent:
        //   `_0`, `_1`, ..., `_N`              — positional
        //   `_<<<<<`                           — outer-chain (any N of `<`)
        //   `_<N`                              — indexed-ascent shortcut
        //   `_N<<<<<` / `_N<M`                 — combined
        //   Sigiled variants:                  `$_0`, `$_<<<<<`, `$_<3`, etc.
        if (buf[p] == '_' && p + 1 < endOffset
            && (buf[p + 1].isDigit() || buf[p + 1] == '<')) {
            p++ // consume `_`
            while (p < endOffset && buf[p].isDigit()) p++
            // Optional `<<<...` outer-chain.
            while (p < endOffset && buf[p] == '<') {
                p++
                // After a SINGLE `<`, allow `<N` indexed-ascent
                // shortcut: digit(s) instead of more `<`s. Continue
                // consuming digits then exit the chevron loop.
                if (p < endOffset && buf[p].isDigit()) {
                    while (p < endOffset && buf[p].isDigit()) p++
                    break
                }
            }
            tokenEnd = p; pos = p
            tokenType = StrykeTokenTypes.BLOCK_PARAM
            return
        }
        // Bare `$_` / `@_` (no extra char) is the topic. Also allow
        // `$_<<<<<` / `$_<3` to flow into the block-param branch
        // above when `<` follows; this branch catches plain `$_`.
        if (buf[p] == '_' && (p + 1 >= endOffset || !buf[p + 1].isLetterOrDigit() && buf[p + 1] != '_' && buf[p + 1] != ':' && buf[p + 1] != '<')) {
            p++
            tokenEnd = p; pos = p
            tokenType = StrykeTokenTypes.TOPIC_VAR
            return
        }
        // Regular identifier (with optional ::name segments). A
        // segment may start with `^` for caret-style special var
        // names (`%main::^HOOK`, `${^OPEN}`, `@main::^CAPTURE`), or
        // with the regular `_` / letter set. Without the `^` branch
        // the lexer split `%main::^HOOK` into `%main::` + `^` +
        // `HOOK`, coloring `^HOOK` as operator + identifier.
        while (p < endOffset) {
            val c = buf[p]
            if (c == '_' || c.isLetterOrDigit()) { p++; continue }
            if (c == ':' && p + 1 < endOffset && buf[p + 1] == ':') {
                // Look ahead past the `::` for a caret-special name.
                if (p + 2 < endOffset && buf[p + 2] == '^') {
                    p += 3 // consume `::^`
                    while (p < endOffset && (buf[p] == '_' || buf[p].isLetterOrDigit())) p++
                    continue
                }
                p += 2
                continue
            }
            break
        }
        // If the sigil consumed nothing past itself, treat as an operator (e.g. `%` modulo).
        if (p == p0 + 1) {
            tokenEnd = p; pos = p
            tokenType = if (sigil == '%') StrykeTokenTypes.OPERATOR else StrykeTokenTypes.SIGIL
            return
        }
        tokenEnd = p; pos = p
        tokenType = varKind(sigil)
    }

    private fun varKind(sigil: Char): IElementType = when (sigil) {
        '$' -> StrykeTokenTypes.SCALAR_VAR
        '@' -> StrykeTokenTypes.ARRAY_VAR
        '%' -> StrykeTokenTypes.HASH_VAR
        else -> StrykeTokenTypes.SCALAR_VAR
    }

    private fun consumeNumber() {
        var p = pos
        var isFloat = false
        while (p < endOffset && (buf[p].isDigit() || buf[p] == '_')) p++
        if (p < endOffset && buf[p] == '.' && p + 1 < endOffset && buf[p + 1].isDigit()) {
            isFloat = true
            p++
            while (p < endOffset && (buf[p].isDigit() || buf[p] == '_')) p++
        }
        if (p < endOffset && (buf[p] == 'e' || buf[p] == 'E')) {
            isFloat = true
            p++
            if (p < endOffset && (buf[p] == '+' || buf[p] == '-')) p++
            while (p < endOffset && buf[p].isDigit()) p++
        }
        tokenEnd = p; pos = p
        tokenType = if (isFloat) StrykeTokenTypes.FLOAT else StrykeTokenTypes.NUMBER
    }

    private fun consumeWord() {
        var p = pos
        while (p < endOffset && (buf[p] == '_' || buf[p].isLetterOrDigit())) p++
        // Stryke implicit block params with outer-chain ascent.
        // When the word starts with `_` and consists ONLY of `_` +
        // optional digits (i.e. `_`, `_0`, `_1`, …, `_N`), extend
        // it with trailing `<<<<<` chevrons or a `<N` indexed-
        // ascent shortcut. Matches the Rust lexer's behavior for
        // bare-form block params (`_<3` ≡ `_<<<`, `_<<<<<` ≡ 5-deep).
        val wordBytes = buf.subSequence(pos, p)
        val isUnderscoreBlockParam = wordBytes.length >= 1
            && wordBytes[0] == '_'
            && (1 until wordBytes.length).all { wordBytes[it].isDigit() }
        if (isUnderscoreBlockParam && p < endOffset && buf[p] == '<') {
            // Consume chevrons. After ONE `<`, if digit(s) follow,
            // accept them as the indexed-ascent form `_<N` and stop.
            while (p < endOffset && buf[p] == '<') {
                p++
                if (p < endOffset && buf[p].isDigit()) {
                    while (p < endOffset && buf[p].isDigit()) p++
                    break
                }
            }
            tokenEnd = p; pos = p
            tokenType = StrykeTokenTypes.BLOCK_PARAM
            return
        }
        // `::` package separator immediately after the segment? Emit
        // just this segment as PACKAGE_NAME and let the lexer's next
        // advance() consume the `::` (which dispatches to the `:`
        // case below and emits PACKAGE_SEPARATOR).
        if (p + 1 < endOffset && buf[p] == ':' && buf[p + 1] == ':') {
            tokenEnd = p; pos = p
            tokenType = StrykeTokenTypes.PACKAGE_NAME
            // Don't propagate fn-intro state through package segments
            // — the FUNCTION_DECL color applies to the LAST segment
            // (the actual sub name), not earlier package qualifiers.
            return
        }
        val word = buf.subSequence(pos, p).toString()
        val wordStart = pos
        tokenEnd = p; pos = p

        // Perl regex / substitution / transliteration prefixes —
        // `m/PATTERN/`, `qr/PATTERN/`, `s/PATTERN/REPLACEMENT/`,
        // `tr/SET1/SET2/`, `y/SET1/SET2/`. Without consuming the
        // delimiter pair as part of this token, the next `/` and any
        // embedded `"` inside the pattern derail the string-state
        // machine — `$q =~ s/"/""/g` would unbalance the `"` quotes
        // and render the rest of the file as string content. Detect
        // the prefix immediately and pull the whole construct in.
        if ((word == "s" || word == "tr" || word == "y" || word == "m" || word == "qr")
            && p < endOffset
            && isRegexDelimiterStart(buf[p])
        ) {
            consumeRegexOrSubstitution(wordStart, word)
            return
        }

        // Distinguish FUNCTION_DECL / FUNCTION_CALL / LABEL from
        // generic IDENTIFIER via context:
        //   - After `fn` / `sub` / `class` / `struct` / `trait` /
        //     `enum` / `method` / `impl` → FUNCTION_DECL.
        //   - Followed by `(` → FUNCTION_CALL.
        //   - Followed by single `:` (not `::`) → LABEL.
        //   - Inside `->{KEY}` or `$h{KEY}` hash subscript → IDENTIFIER
        //     (force bareword; `state`/`my`/`for`/`if` etc. are valid
        //     hash keys and must not render as keywords).
        //   - Followed by `=>` (fat-comma autoquote) → IDENTIFIER.
        val classified = classifyWord(word)
        val inHashKey = isHashKeyContext(wordStart) || nextIsFatArrow(p)
        tokenType = when {
            inHashKey -> StrykeTokenTypes.IDENTIFIER
            // After `fn` / `sub` / `class` / `struct` / `trait` / `enum`
            // / `method` / `impl`, the next word is always the
            // declared name — even if its spelling collides with a
            // keyword. `trait T { fn state; fn transition }` should
            // mark `state` and `transition` as FUNCTION_DECL, not
            // the `state` decl-keyword. Force the override regardless
            // of `classified`.
            lastWasFnIntro -> StrykeTokenTypes.FUNCTION_DECL
            classified == StrykeTokenTypes.IDENTIFIER && peekNextNonSpace(p) == '(' ->
                StrykeTokenTypes.FUNCTION_CALL
            classified == StrykeTokenTypes.IDENTIFIER && nextIsSingleColon(p) ->
                StrykeTokenTypes.LABEL
            else -> classified
        }
        // Update fn-intro state for the NEXT consumeWord call. Skip the
        // update when this word is itself a hash-key bareword (it can't
        // introduce a fn body).
        lastWasFnIntro = !inHashKey && classified == StrykeTokenTypes.FN_KEYWORD
    }

    /**
     * True when the word at `wordStart` sits inside a hash-subscript:
     * `$h{KEY}`, `@h{K1,K2}`, `%h{KEY}`, `$ref->{KEY}`, `${expr}{KEY}`,
     * etc. Used to keep Perl-keyword spellings (`state`, `my`, `for`,
     * `if`, `last`, ...) classified as bareword IDENTIFIER inside hash
     * keys, not as keyword tokens.
     */
    private fun isHashKeyContext(wordStart: Int): Boolean {
        var i = wordStart - 1
        while (i >= 0 && (buf[i] == ' ' || buf[i] == '\t')) i--
        if (i < 0 || buf[i] != '{') return false
        // Char immediately before the `{`.
        if (i == 0) return false
        val before = buf[i - 1]
        if (before == '>' && i >= 2 && buf[i - 2] == '-') return true // `->{`
        if (before == '}') return true // `${expr}{KEY}` chained
        if (before.isLetterOrDigit() || before == '_') {
            // Walk back to start of the ident run.
            var j = i - 1
            while (j > 0 && (buf[j - 1].isLetterOrDigit() || buf[j - 1] == '_')) j--
            // If the char immediately before the run is `$`/`@`/`%`,
            // this is a sigil-var hash subscript (`$h{`, `@h{`, `%h{`).
            if (j > 0) {
                val sigil = buf[j - 1]
                if (sigil == '$' || sigil == '@' || sigil == '%') return true
            }
        }
        return false
    }

    /**
     * True when the next non-whitespace token starting at `from` is the
     * `=>` fat-comma operator. Fat-comma autoquotes the preceding
     * bareword, so `state => 1` makes `state` a key, not a keyword.
     */
    private fun nextIsFatArrow(from: Int): Boolean {
        var i = from
        while (i < endOffset && (buf[i] == ' ' || buf[i] == '\t')) i++
        return i + 1 < endOffset && buf[i] == '=' && buf[i + 1] == '>'
    }

    /** Peek the next non-whitespace char (newlines included as whitespace). */
    private fun peekNextNonSpace(from: Int): Char? {
        var i = from
        while (i < endOffset) {
            val c = buf[i]
            if (c != ' ' && c != '\t') return c
            i++
        }
        return null
    }

    /** True if the next non-whitespace char is `:` not followed by another `:`. */
    private fun nextIsSingleColon(from: Int): Boolean {
        var i = from
        while (i < endOffset && (buf[i] == ' ' || buf[i] == '\t')) i++
        if (i >= endOffset || buf[i] != ':') return false
        return i + 1 >= endOffset || buf[i + 1] != ':'
    }

    /**
     * Valid delimiter starts for `m/.../`, `qr/.../`, `s/.../.../`,
     * `tr/.../.../`, `y/.../.../`. Perl accepts any non-alphanumeric;
     * we cover the common ones plus paired-bracket forms.
     */
    private fun isRegexDelimiterStart(c: Char): Boolean =
        c == '/' || c == '!' || c == '#' || c == '|' || c == '{' ||
        c == '[' || c == '(' || c == '<' || c == '"' || c == '\''

    /**
     * Consume a `m`/`qr`/`s`/`tr`/`y` operator including its delimiter
     * pair(s) and optional trailing flag letters, emitting one REGEX
     * token that spans the whole construct. Critical for handling
     * `s/"/""/g` — the embedded `"` characters are NOT string quotes
     * here; the substitution is one atomic lexer unit.
     *
     * Two-segment ops (`s`, `tr`, `y`) take TWO pattern halves.
     * Single-segment ops (`m`, `qr`) take ONE.
     * Paired-bracket delimiters (`{...}`, `[...]`, `(...)`, `<...>`)
     * use the matching close char as the segment terminator.
     */
    private fun consumeRegexOrSubstitution(wordStart: Int, op: String) {
        val twoSegment = op == "s" || op == "tr" || op == "y"
        val open = buf[pos]
        val close = matchingDelimiterClose(open)
        var p = pos + 1
        var depth = if (open == close) 0 else 1
        // First segment.
        while (p < endOffset) {
            val c = buf[p]
            if (c == '\\' && p + 1 < endOffset) { p += 2; continue }
            if (open != close && c == open) { depth++; p++; continue }
            if (c == close) {
                if (open == close) { p++; break }
                depth--
                if (depth == 0) { p++; break }
            }
            if (c == '\n' && open == '/') break // best-effort: don't run across lines
            p++
        }
        // Second segment for `s`, `tr`, `y`.
        if (twoSegment) {
            // For paired-bracket delimiters, the second segment may have
            // its own opener/closer (e.g. `s{foo}{bar}`); for symmetric
            // delimiters (`/`, `!`, `|`), the same char re-opens.
            if (open != close && p < endOffset) {
                // Skip any whitespace between bracket-paired segments.
                while (p < endOffset && (buf[p] == ' ' || buf[p] == '\t')) p++
                if (p < endOffset && (isRegexDelimiterStart(buf[p]))) {
                    val open2 = buf[p]
                    val close2 = matchingDelimiterClose(open2)
                    p++
                    var d2 = if (open2 == close2) 0 else 1
                    while (p < endOffset) {
                        val c = buf[p]
                        if (c == '\\' && p + 1 < endOffset) { p += 2; continue }
                        if (open2 != close2 && c == open2) { d2++; p++; continue }
                        if (c == close2) {
                            if (open2 == close2) { p++; break }
                            d2--
                            if (d2 == 0) { p++; break }
                        }
                        if (c == '\n' && open2 == '/') break
                        p++
                    }
                }
            } else {
                // Symmetric delimiter — second segment ends at the same char.
                while (p < endOffset) {
                    val c = buf[p]
                    if (c == '\\' && p + 1 < endOffset) { p += 2; continue }
                    if (c == close) { p++; break }
                    if (c == '\n' && open == '/') break
                    p++
                }
            }
        }
        // Optional trailing flag letters (g, i, m, s, x, e, r, …).
        while (p < endOffset && buf[p].isLetter()) p++
        tokenStart = wordStart
        tokenEnd = p
        pos = p
        tokenType = StrykeTokenTypes.REGEX
    }

    /** Paired-bracket delimiter → matching close char; otherwise the same char. */
    private fun matchingDelimiterClose(open: Char): Char = when (open) {
        '{' -> '}'
        '[' -> ']'
        '(' -> ')'
        '<' -> '>'
        else -> open
    }

    private fun consumeRegexOrSlash() {
        val prev = lastNonSpaceBefore(pos)
        val looksLikeRegex = prev == null || prev in REGEX_ANCHORS
        if (!looksLikeRegex) {
            emit(1, StrykeTokenTypes.OPERATOR); return
        }
        var p = pos + 1
        var bracket = 0
        while (p < endOffset) {
            val c = buf[p]
            if (c == '\\' && p + 1 < endOffset) { p += 2; continue }
            if (c == '[') bracket++
            if (c == ']' && bracket > 0) bracket--
            if (c == '/' && bracket == 0) { p++; break }
            if (c == '\n') break
            p++
        }
        // Emit just the `/pattern/` as REGEX. If letters follow (the
        // flags), set state so the next advance() emits them as one
        // REGEX_FLAGS token — distinct color category in the picker.
        tokenEnd = p; pos = p
        tokenType = StrykeTokenTypes.REGEX
        if (p < endOffset && buf[p].isLetter()) {
            state = STATE_REGEX_FLAGS_PENDING
        }
    }

    private fun lastNonSpaceBefore(p: Int): Char? {
        var i = p - 1
        while (i >= 0) {
            val c = buf[i]
            if (c != ' ' && c != '\t') return c
            i--
        }
        return null
    }

    private fun classifyWord(word: String): IElementType {
        // Bare `_N` outside sigil context = block param
        if (word.length > 1 && word[0] == '_' && word.substring(1).all { it.isDigit() }) {
            return StrykeTokenTypes.BLOCK_PARAM
        }
        if (word == "_") return StrykeTokenTypes.TOPIC_VAR
        return when (word) {
            in DECL_KEYWORDS -> StrykeTokenTypes.DECL_KEYWORD
            in FN_KEYWORDS -> StrykeTokenTypes.FN_KEYWORD
            in CONTROL_KEYWORDS -> StrykeTokenTypes.CONTROL_KEYWORD
            in PHASE_KEYWORDS -> StrykeTokenTypes.PHASE_KEYWORD
            in WORD_OPERATORS -> StrykeTokenTypes.WORD_OPERATOR
            in BOOLEANS -> StrykeTokenTypes.BOOLEAN
            "undef" -> StrykeTokenTypes.UNDEF
            in PARALLEL_BUILTINS -> StrykeTokenTypes.BUILTIN_PARALLEL
            in BUILTINS -> StrykeTokenTypes.BUILTIN
            else -> StrykeTokenTypes.IDENTIFIER
        }
    }

    private fun isOperatorChar(c: Char): Boolean =
        c in "+-*/%=<>!&|^~?:\\"

    private fun isSpecialVarChar(c: Char): Boolean = c in SPECIAL_VAR_CHARS

    companion object {
        // Lexer state machine for `"..."` interpolation. IntelliJ feeds the
        // state back to the lexer on incremental relexing — without these,
        // a restart mid-string would lose context and treat `#` as a comment.
        const val STATE_NORMAL = 0
        const val STATE_IN_DQ_STRING = 1          // resume scanning string literal
        const val STATE_IN_DQ_INTERP_START = 2    // next token is the `#{` opener
        const val STATE_IN_DQ_INTERP = 3          // inside the `#{EXPR}` expression
        const val STATE_IN_DQ_SIGIL_VAR = 4       // next token is a `$var` / `@arr` / `%h` inside a string
        const val STATE_IN_DQ_FORMAT = 5          // next token is a `%d` / `%10.2f` printf format spec inside a string
        const val STATE_REGEX_FLAGS_PENDING = 6   // next token is the letter run after `/pattern/` (e.g. `i`, `g`, `igs`)
        const val STATE_IN_DQ_ARRAY_INTERP_START = 7  // next token is the `@{[` opener (Perl-style array-ref interp)
        const val STATE_IN_DQ_ARRAY_INTERP = 8        // inside the `@{[ EXPR ]}` expression (close on `]}`)

        private val FORMAT_FLAGS = setOf('-', '+', '0', ' ', '#')
        private val FORMAT_LENGTH = setOf('l', 'h', 'L', 'q', 'z', 'j', 't')
        private val FORMAT_CONV = setOf(
            'd', 'i', 'o', 'u', 'x', 'X',
            'e', 'E', 'f', 'F', 'g', 'G',
            's', 'S', 'c', 'C', 'p', 'n',
            'b', 'a', 'A', 'v',
        )

        private val DECL_KEYWORDS = setOf(
            "my", "our", "local", "state", "use", "no", "package", "require",
            "has", "pub", "priv", "in", "is", "as",
        )
        private val FN_KEYWORDS = setOf(
            "fn", "sub", "method", "class", "trait", "struct", "enum", "impl", "extends",
        )
        private val CONTROL_KEYWORDS = setOf(
            "return", "if", "elsif", "else", "unless", "while", "until", "for", "foreach",
            "do", "last", "next", "redo", "given", "when", "default", "die", "eval",
            "try", "catch", "finally",
        )
        private val PHASE_KEYWORDS = setOf(
            "BEGIN", "END", "INIT", "CHECK", "UNITCHECK", "BUILD", "DESTROY",
        )
        private val WORD_OPERATORS = setOf(
            "and", "or", "not", "xor", "cmp",
            "eq", "ne", "lt", "le", "gt", "ge", "x",
        )
        private val BOOLEANS = setOf("true", "false")

        /** Parallel primitives — get their own color slot. */
        private val PARALLEL_BUILTINS = setOf(
            "pmap", "pgrep", "pfor", "pforeach", "pflat_map",
            "pmaps", "pgreps", "pflat_maps",
            "par_fetch", "par_each", "par_run", "par_apply",
            "channel", "spawn", "await", "async",
        )

        private val BUILTINS = setOf(
            "p", "ep", "say", "print", "printf", "warn", "len", "scalar", "keys", "values",
            "each", "push", "pop", "shift", "unshift", "splice", "reverse", "sort", "join",
            "split", "map", "grep", "fi", "reduce", "fold", "filter", "take", "skip",
            "tap", "open", "close", "read", "write", "chomp", "chop", "exists", "defined",
            "delete", "wantarray", "ref", "bless", "tie", "tied", "untie",
            "json_encode", "json_decode", "tj", "fj", "yaml_encode", "yaml_decode",
            "xml_encode", "xml_decode", "csv_encode", "csv_decode", "toml_encode", "toml_decode",
            "fetch", "http_request", "hr", "serve", "websocket",
            "jwt_encode", "jwt_decode", "sha256", "sha512", "md5", "hmac",
            "ai", "embedding", "complete",
            "time", "sleep", "log_json", "set", "pipeline", "to_set", "uniq",
            "regex", "match", "subst", "lc", "uc", "lcfirst", "ucfirst", "sprintf",
            "int", "abs", "sqrt", "exp", "log", "sin", "cos", "tan", "atan2", "rand",
            "srand", "min", "max", "sum", "mean", "median", "stddev", "variance",
            "snake_case", "sc", "camel_case", "cc", "pascal_case", "kebab_case",
            "spurt", "slurp", "exists_file", "exists_dir", "mkdir_p", "rmdir_r",
            "now_ns", "td_add",
        )
        private val REGEX_ANCHORS = setOf(',', '(', '=', ';', '{', '|', '&', '~', '!', '?')
        private val SPECIAL_VAR_CHARS = setOf(
            '!', '@', '$', ',', ';', '/', '\\', '"', '\'', '&', '`', '+',
            '.', '?', '<', '>', '(', ')', '[', ']', '~', '^', '0', '1', '2', '3',
            '4', '5', '6', '7', '8', '9',
        )
    }
}
