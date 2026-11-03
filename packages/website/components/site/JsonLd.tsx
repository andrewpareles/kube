import { getJsonLdScriptText } from '@/lib/siteJsonLd'

// JSON-LD stays next to the visible page that owns the same facts.
export const JsonLd = ({ value }: { value: object }) => (
    <script
        type="application/ld+json"
        dangerouslySetInnerHTML={{ __html: getJsonLdScriptText(value) }}
    />
)
