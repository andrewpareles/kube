"use server";

import { getUser } from "@/lib/siteUser";
import { welcomeIfNewAccount } from "@/lib/welcomeEmail";

// Google One Tap signs in from the browser, so it asks for the welcome email here once the session exists.
export const welcomeNewAccount = async () => {
	welcomeIfNewAccount(await getUser());
};
