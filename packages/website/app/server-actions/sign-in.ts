"use server";

import { revalidatePath } from "next/cache";
import { redirect } from "next/navigation";
import { getRedirectToUrl } from "@/app/login/loginRedirect";
import { getAuthErrorMessage } from "@/lib/siteAuthErrors";
import { supabaseOnServer } from "@/supabase/supabaseOnServer";

/**
 * Signs in on the server, so the auth cookie is set in the same response that renders the site again
 * for the new user. The header then arrives already showing their name and balances, instead of the
 * page keeping the signed-out header it was first drawn with.
 */
export const signIn = async ({ email, password, next }: { email: string, password: string, next: string }): Promise<{ error: string }> => {
	const supabase = await supabaseOnServer();
	const { error } = await supabase.auth.signInWithPassword({ email, password });
	if (error) {
		return { error: getAuthErrorMessage({ error }) };
	}
	revalidatePath("/", "layout");
	redirect(getRedirectToUrl(next));
};
