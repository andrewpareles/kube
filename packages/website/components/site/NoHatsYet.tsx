import Link from 'next/link'
import { CATALOG_PATH } from '@/lib/siteNavigation'

// Your own empty hat lists point you to where hats come from.
export const NoHatsYet = () => (
    <p className="text-sm text-gray-500">No hats yet. <Link href={CATALOG_PATH} className="font-medium text-kube-blue hover:underline">Explore the Hat Catalog.</Link></p>
)
