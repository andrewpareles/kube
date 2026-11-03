import { createHash, randomBytes } from "crypto";


export const hashDesktopAuthToken = (authStr: string) => {
	return createHash("sha256")
		.update(authStr)
		.digest("hex");
};

// Mint a desktop play token and the hash stored for authentication.
export const createDesktopAuth = () => {
	const authStr = randomBytes(32).toString("base64url");
	const hash = hashDesktopAuthToken(authStr);
	return { authStr, hash };
};
