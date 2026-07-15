Code Review Instructions
Purpose
These instructions guide Copilot code review for all pull requests in this repository. Prioritize findings in this order: security, correctness, reliability, performance, maintainability.
Security
    • Check for hardcoded secrets, API keys, tokens, passwords, or connection strings
    • Flag SQL injection risks; require parameterized queries, never string concatenation
    • Flag XSS risks: unescaped output, innerHTML or dangerouslySetInnerHTML with user data
    • Flag command injection, path traversal, and unsafe deserialization of untrusted input
    • Verify authentication and authorization checks on new or changed endpoints
    • Verify all external input is validated and sanitized at trust boundaries
    • Flag weak cryptography: MD5/SHA-1 for passwords, hardcoded keys or IVs
    • Flag SSRF risks: outbound requests to user-controlled URLs
    • Flag sensitive data (passwords, tokens, PII) written to logs or error messages
    • Flag missing or unsafe cookie settings (httpOnly, secure, sameSite) and overly permissive CORS
    • Question newly added dependencies: necessary, maintained, and from a trusted source?
Correctness and logic
    • Verify boundary conditions: off-by-one errors, empty collections, first/last elements
    • Check null/undefined handling on every new code path
    • Flag logic that contradicts the function name, comments, or documentation
    • Check inverted or incomplete conditionals: && vs ||, missing negation, unreachable branches
    • Flag race conditions and shared mutable state in concurrent code
    • Check date/time handling: timezones, DST, month indexing
    • Flag floating-point equality comparisons and integer overflow risks
    • Verify loops terminate and recursion has a base case
Errors and reliability
    • Flag swallowed exceptions and empty catch blocks
    • Require specific exception types, not broad exception handling
    • Verify resources (files, connections, locks) are released on all paths, including error paths
    • Check that failing external calls (HTTP, database, queue) have timeouts and are handled
    • Flag error messages that leak internals (stack traces, queries) to end users
Performance
    • Identify N+1 query patterns and queries inside loops
    • Flag unbounded result sets: missing pagination or limits
    • Spot repeated computation of the same value
Tests
    • New logic requires tests; flag changed behavior with unchanged tests
    • Tests should cover error paths and edge cases, not just the happy path
    • Flag tests that assert nothing or can never fail
Review style
    • Be specific and actionable; include a suggested fix where possible
    • Briefly explain why an issue matters
    • Do not comment on formatting or style already enforced by linters
    • Ask a clarifying question when code intent is unclear instead of guessing
Maintaining these instructions
    • If you find a significant issue of a type not covered by these instructions, or the same issue class in 3+ places in one review, note it in the overall review and recommend exact wording for a follow-up update to this file.
