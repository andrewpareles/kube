"use server";

import { db } from "@/lib/db";
import { getUsernameFilterProblem } from "@/lib/siteUsernameFilter";
import { getUsernameProblem, type UsernameAvailability } from "@/lib/siteUsernames";
import { userProfiles } from "@/supabase/migrations/schema";
import { sql } from "drizzle-orm";

/**
 * Whether a name is still free for somebody changing their name to take.
 *
 * Case is ignored, the same way the unique index on the table ignores it, so a name this calls free
 * is one they can actually go on to hold. The word list is checked here as well as the way the
 * name is written, which is why this has to run on the server and why the answer says which of the
 * two is wrong rather than just saying no.
 */
export const getUsernameAvailability = async (username: string): Promise<UsernameAvailability> => {
	const problem = getUsernameProblem(username) ?? getUsernameFilterProblem(username);
	if (problem !== null) {
		return { type: "problem", problem };
	}

	const [profileWithName] = await db
		.select({ username: userProfiles.username })
		.from(userProfiles)
		.where(sql`lower(${userProfiles.username}) = ${username.toLowerCase()}`)
		.limit(1);

	return profileWithName === undefined ? { type: "available" } : { type: "taken" };
};
