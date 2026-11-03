import Link from 'next/link'
import { ArrowLeft } from 'lucide-react'

export const SiteBackLink = ({ href, label }: { href: string; label: string }) => (
    <Link href={href} aria-label={`Back To ${label}`} className="mb-6 inline-flex items-center gap-2 text-xs text-foreground">
        <ArrowLeft className="size-4" aria-hidden="true" />
        {label}
    </Link>
)
