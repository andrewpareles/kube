"use server";

import { getAuthErrorMessage } from "@/lib/siteAuthErrors";
import { passwordMinLength } from "@/lib/sitePasswords";
import { supabaseOnServer } from "@/supabase/supabaseOnServer";

/**
 * Makes the account.
 *
 * Sign up goes through here rather than straight from the page so the password length is checked
 * somewhere the page cannot skip.
 *
 * Supabase's own sign up endpoint is still open to anything that calls it directly, so this is the
 * gate for people using the site, not a guarantee about the table.
 */
export const createAccount = async ({ email, password, emailRedirectTo }: {
	email: string
	password: string
	emailRedirectTo: string
}): Promise<{ type: "checkEmail" } | { type: "error", error: string }> => {
	if (password.length < passwordMinLength) {
		return { type: "error", error: `Your password needs at least ${passwordMinLength} characters.` };
	}

	const supabase = await supabaseOnServer();
	const { error } = await supabase.auth.signUp({
		email,
		password,
		options: {
			emailRedirectTo, // confirmation email redirects here
		},
	});
	if (error) {
		return { type: "error", error: getAuthErrorMessage({ error }) };
	}
	return { type: "checkEmail" };
};
