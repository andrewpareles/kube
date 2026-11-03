"use server";

import { and, eq, sql } from "drizzle-orm";
import { getUsernameAvailability } from "@/app/server-actions/username-available";
import { db } from "@/lib/db";
import { requireUser } from "@/lib/siteUser";
import { userProfiles } from "@/supabase/migrations/schema";

/**
 * Renames the signed-in account, once: only an account still called Guest-<user_id> can pick a name.
 * The name is checked again here, since what the field showed while they were typing is only advice
 * by the time the button is pressed. The check only allows letters and digits, so nobody can pick a
 * Guest- name.
 *
 * The page is not refreshed here. The welcome shown after the rename lives in the banner, and a
 * refresh would take the banner away with it, so the page refreshes once the welcome is closed.
 */
export const changeUsername = async (username: string): Promise<{ type: "changed" } | { type: "error", error: string }> => {
	const user = await requireUser();

	const availability = await getUsernameAvailability(username);
	if (availability.type === "taken") {
		return { type: "error", error: `${username} is taken.` };
	}
	if (availability.type === "problem") {
		return { type: "error", error: availability.problem };
	}

	const renamedProfiles = await db
		.update(userProfiles)
		.set({ username })
		.where(and(eq(userProfiles.id, user.id), sql`${userProfiles.username} = 'Guest-' || ${userProfiles.userId}`))
		.returning({ id: userProfiles.id });
	if (renamedProfiles.length === 0) {
		return { type: "error", error: "You already picked your username." };
	}
	return { type: "changed" };
};
