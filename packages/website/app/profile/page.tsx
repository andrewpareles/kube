import { eq } from 'drizzle-orm'
import type { Metadata } from 'next'
import { notFound, redirect } from 'next/navigation'
import { getLoginUrl } from '@/app/deeplink/deeplinkFlowConsts'
import { db } from '@/lib/db'
import { getNoIndexPageMetadata } from '@/lib/siteMetadata'
import { getUser } from '@/lib/siteUser'
import { userProfiles } from '@/supabase/migrations/schema'

export const metadata: Metadata = getNoIndexPageMetadata({
    title: 'Your Profile',
    description: null,
})

const ProfilePage = async () => {
    const user = await getUser()

    if (user === null) {
        redirect(getLoginUrl({ next: '/profile' }))
    }

    const [profile] = await db
        .select({
            userId: userProfiles.userId,
        })
        .from(userProfiles)
        .where(eq(userProfiles.id, user.id))
        .limit(1)

    if (!profile) {
        notFound()
    }

    redirect(`/profile/${profile.userId}`)
}

export default ProfilePage
