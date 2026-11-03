import { Info, CircleUserRound } from 'lucide-react'
import { HoverTooltip } from '@/components/site/HoverTooltip'

export const HumanWrittenNote = () => (
    <HoverTooltip
        side="top"
        className="flex items-center gap-1.5"
        trigger={<span className="inline-flex shrink-0 self-start text-gray-400 transition-colors hover:text-gray-600" aria-label="Descriptions are always written by a human (the creator)."><Info className="size-3.5" /></span>}
    >
        <CircleUserRound className="size-3.5" aria-hidden="true" />
        Descriptions are always written by a human (the creator).
    </HoverTooltip>
)
