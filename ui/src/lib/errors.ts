/**
 * Converts raw technical error messages into user-friendly text.
 */
export function friendlyError(err: unknown): string {
  const raw = typeof err === "string" ? err : (err as any)?.message || String(err);

  // Auth errors
  if (raw.includes("UserNotFoundException") || raw.includes("User does not exist"))
    return "Account not found. Check your email or employee ID.";
  if (raw.includes("NotAuthorizedException") || raw.includes("Incorrect username or password"))
    return "Wrong password. Please try again.";
  if (raw.includes("Employee ID not found"))
    return "Employee ID not found. Check the ID and try again.";
  if (raw.includes("not authenticated") || raw.includes("token refresh failed"))
    return "Session expired. Please sign in again.";
  if (raw.includes("NEW_PASSWORD_REQUIRED"))
    return "Please set a new password to continue.";
  if (raw.includes("password") && raw.includes("too short"))
    return "Password must be at least 8 characters.";
  if (raw.includes("InvalidPasswordException"))
    return "Password must include uppercase, lowercase, and a number.";

  // Token storage errors
  if (raw.includes("keyring") || raw.includes("too big") || raw.includes("Credential"))
    return "Unable to save login. Please try again.";

  // Network errors
  if (raw.includes("network error") || raw.includes("Failed to fetch") || raw.includes("ECONNREFUSED"))
    return "Can't reach the server. Check your internet connection.";
  if (raw.includes("timeout"))
    return "Request timed out. Please try again.";

  // Timer errors
  if (raw.includes("already signed in"))
    return "Timer is already running.";
  if (raw.includes("not signed in") || raw.includes("not currently signed in"))
    return "No active timer to stop.";
  if (raw.includes("description is required"))
    return "Please enter what you're working on.";

  // API errors
  if (raw.includes("401") || raw.includes("Unauthorized"))
    return "Session expired. Please sign in again.";
  if (raw.includes("403") || raw.includes("Forbidden"))
    return "You don't have permission for this action.";
  if (raw.includes("500") || raw.includes("Internal server error"))
    return "Something went wrong. Please try again.";

  // If the message is already short and readable, use it — but
  // strip anything that could look like markup. React's current
  // renderers auto-escape, so `<img onerror>` displays as literal
  // text today, not script. This is defense-in-depth against a
  // future refactor that switches to dangerouslySetInnerHTML or
  // injects the error into a template that doesn't escape.
  if (raw.length < 80 && !raw.includes("::") && !raw.includes("Error:"))
    return sanitizeDisplayString(raw);

  // Fallback
  return "Something went wrong. Please try again.";
}

// sanitizeDisplayString removes characters that have no legitimate
// place in a user-facing error: HTML angle brackets, NULs, and
// non-printable control chars other than \n and \t. Leaves the
// string otherwise intact.
function sanitizeDisplayString(s: string): string {
  return s
    .replace(/[<>]/g, "")       // strip angle brackets
    .replace(/[\x00-\x08\x0B\x0C\x0E-\x1F]/g, ""); // keep \t, \n, \r
}
