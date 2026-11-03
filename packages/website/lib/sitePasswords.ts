/**
 * How short a password may be. The sign up form asks for this and the action that makes the account
 * checks it again, since the form's own check is only advice by the time the button is pressed.
 *
 * Signing in does not hold anybody to it: an account made before this ruled would be locked out of
 * its own password.
 */
export const passwordMinLength = 8
