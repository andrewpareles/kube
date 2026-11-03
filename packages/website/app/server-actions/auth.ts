"use server";

import { db } from "@/lib/db";
import { getUser } from "@/lib/siteUser";
import { userDesktopPlayAuth } from "@/supabase/migrations/schema";
import { eq } from "drizzle-orm";

// Clear desktop play sessions for the user.
export const clearDesktopAuthSessions = async () => {
	const user = await getUser();
	if (user === null) {
		return;
	}

  await db
    .delete(userDesktopPlayAuth)
    .where(eq(userDesktopPlayAuth.profileId, user.id));
};
