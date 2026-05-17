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
        // Resume scanning the suffix of an interrupted `"..."` (after an
        // interpolation closes). Picks up exactly where consumeString left
        // off — no missing `#`-as-comment misinterpretation.
        if (state == STATE_IN_DQ_STRING) {
            consumeDoubleStringContinuation()
            return
        }
        val c = buf[pos]
        when {
            c == '#' -> consumeLineComment()
            c == '\n' || c == '\r' || c == ' ' || c == '\t' -> consumeWhitespace()
            c == '"' -> consumeDoubleQuoteString()
            c == '\'' -> consumeString('\'')
            c == '`' -> consumeString('`')
            // Heredoc: <<EOT, <<'EOT', <<"EOT", <<~EOT
            c == '<' && peek(1) == '<' && isHeredocStart(2) -> consumeHeredoc()
            c == '-' && peek(1) == '>' -> emit(2, StrykeTokenTypes.ARROW_OP)
            c == '=' && peek(1) == '>' -> emit(2, StrykeTokenTypes.FAT_COMMA)
            c == '|' && peek(1) == '>' && peek(2) == '>' -> emit(3, StrykeTokenTypes.PIPE)
            c == '|' && peek(1) == '>' -> emit(2, StrykeTokenTypes.PIPE)
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
        // pos is at the opening `"`. Scan forward until we hit either the
        // closing `"` or an interpolation start `#{`.
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
            p++
        }
        tokenEnd = p
        pos = p
        state = STATE_NORMAL
        tokenType = StrykeTokenTypes.STRING
    }

    /**
     * Continuation of a double-quoted string after an interpolation `}`
     * closes. Behaves like [consumeDoubleQuoteString] but doesn't skip the
     * opening `"` (there isn't one — we're mid-string).
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
            p++
        }
        tokenEnd = p
        pos = p
        state = STATE_NORMAL
        tokenType = StrykeTokenTypes.STRING
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
            c == '#' -> consumeLineComment()
            c == '\n' || c == '\r' || c == ' ' || c == '\t' -> consumeWhitespace()
            c == '"' -> consumeDoubleQuoteString()
            c == '\'' -> consumeString('\'')
            c == '`' -> consumeString('`')
            c == '<' && peek(1) == '<' && isHeredocStart(2) -> consumeHeredoc()
            c == '-' && peek(1) == '>' -> emit(2, StrykeTokenTypes.ARROW_OP)
            c == '=' && peek(1) == '>' -> emit(2, StrykeTokenTypes.FAT_COMMA)
            c == '|' && peek(1) == '>' && peek(2) == '>' -> emit(3, StrykeTokenTypes.PIPE)
            c == '|' && peek(1) == '>' -> emit(2, StrykeTokenTypes.PIPE)
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
            isOperatorChar(c) -> emit(1, StrykeTokenTypes.OPERATOR)
            else -> emit(1, TokenType.BAD_CHARACTER)
        }
    }

    private var interpBraceDepth: Int = 0

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
            val name = "$sigil${buf[p]}"
            p++
            tokenEnd = p; pos = p
            tokenType = when {
                name == "\$_" || name == "@_" -> StrykeTokenTypes.TOPIC_VAR
                else -> StrykeTokenTypes.SPECIAL_VAR
            }
            return
        }
        // Underscore positional block params: `_0`, `_1`, ..., `$_0`, `@_0` (less common)
        if (buf[p] == '_' && p + 1 < endOffset && buf[p + 1].isDigit()) {
            p += 2
            while (p < endOffset && buf[p].isDigit()) p++
            tokenEnd = p; pos = p
            tokenType = StrykeTokenTypes.BLOCK_PARAM
            return
        }
        // Bare `$_` / `@_` (no extra char) is the topic.
        if (buf[p] == '_' && (p + 1 >= endOffset || !buf[p + 1].isLetterOrDigit() && buf[p + 1] != '_' && buf[p + 1] != ':')) {
            p++
            tokenEnd = p; pos = p
            tokenType = StrykeTokenTypes.TOPIC_VAR
            return
        }
        // Regular identifier (with optional ::name segments).
        while (p < endOffset) {
            val c = buf[p]
            if (c == '_' || c.isLetterOrDigit()) { p++; continue }
            if (c == ':' && p + 1 < endOffset && buf[p + 1] == ':') { p += 2; continue }
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
        // Look ahead for `::` — package path
        var pkgEnd = p
        var hadPkg = false
        while (pkgEnd + 1 < endOffset && buf[pkgEnd] == ':' && buf[pkgEnd + 1] == ':') {
            hadPkg = true
            pkgEnd += 2
            while (pkgEnd < endOffset && (buf[pkgEnd] == '_' || buf[pkgEnd].isLetterOrDigit())) pkgEnd++
        }
        if (hadPkg) {
            // Emit just the first segment now; the lexer will re-enter for the `::` next.
            // To keep it simple, emit the *entire* package path as PACKAGE_NAME — the user
            // can rebind that color separately and the `::` separators become part of it.
            tokenEnd = pkgEnd; pos = pkgEnd
            tokenType = StrykeTokenTypes.PACKAGE_NAME
            return
        }
        val word = buf.subSequence(pos, p).toString()
        tokenEnd = p; pos = p
        tokenType = classifyWord(word)
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
        val flagsStart = p
        while (p < endOffset && buf[p].isLetter()) p++
        if (p > flagsStart) {
            tokenEnd = flagsStart; pos = flagsStart
            tokenType = StrykeTokenTypes.REGEX
            // Note: the flags get emitted on the next advance().
            // Push the lexer to recognise them as REGEX_FLAGS.
            return
        }
        tokenEnd = p; pos = p
        tokenType = StrykeTokenTypes.REGEX
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
