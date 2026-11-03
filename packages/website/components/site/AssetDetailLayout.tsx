import type { ReactNode } from 'react'

// Every asset page has the same shape: the preview on the left with its footer under it, and its controls, if
// any, beside it.
export const AssetDetailLayout = ({ preview, controls, footer }: { preview: ReactNode, controls: ReactNode, footer: ReactNode }) => <div className="grid items-stretch gap-x-10 gap-y-6 md:grid-cols-[minmax(0,1fr)_18rem]">
    <div className="min-w-0">{preview}{footer !== null && <div className="mt-6">{footer}</div>}</div>
    <div className="min-w-0">{controls}</div>
</div>
