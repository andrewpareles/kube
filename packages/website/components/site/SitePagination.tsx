import Link from 'next/link'
import { ChevronLeft, ChevronRight } from 'lucide-react'
import { cn } from '@/shadcn/lib/utils'
import { buttonVariants } from '@/shadcn/components/ui/button'
import { Pagination, PaginationContent, PaginationEllipsis, PaginationItem } from '@/shadcn/components/ui/pagination'

// The first and last page, and the ones beside the current page. A gap of one page shows that page instead of an ellipsis.
export const getVisiblePages = (page: number, pageCount: number): (number | 'ellipsis')[] => {
    const shownPages = [...new Set([1, page - 1, page, page + 1, pageCount])].filter(shownPage => shownPage >= 1 && shownPage <= pageCount).sort((a, b) => a - b)
    return shownPages.flatMap((shownPage, index) => {
        const gap = index === 0 ? 1 : shownPage - shownPages[index - 1]
        return gap === 1 ? [shownPage] : gap === 2 ? [shownPage - 1, shownPage] : ['ellipsis' as const, shownPage]
    })
}

export const SitePagination = ({ page, pageCount, hrefOfPage, label, className }: { page: number, pageCount: number, hrefOfPage: (page: number) => string, label: string, className?: string }) => {
    if (pageCount <= 1) return null
    const edgeClassName = cn(buttonVariants({ variant: 'ghost' }), 'gap-1 px-2.5')

    return (
        <Pagination aria-label={label} className={cn('mx-0 justify-start', className)}>
            <PaginationContent>
                <PaginationItem>
                    {page > 1
                        ? <Link href={hrefOfPage(Math.min(page - 1, pageCount))} aria-label="Go to previous page" className={edgeClassName}><ChevronLeft className="size-4" /><span className="hidden sm:block">Previous</span></Link>
                        : <span aria-disabled="true" className={cn(edgeClassName, 'pointer-events-none opacity-50')}><ChevronLeft className="size-4" /><span className="hidden sm:block">Previous</span></span>}
                </PaginationItem>
                {getVisiblePages(page, pageCount).map((shownPage, index) => (
                    <PaginationItem key={shownPage === 'ellipsis' ? `ellipsis-${index}` : shownPage}>
                        {shownPage === 'ellipsis'
                            ? <PaginationEllipsis />
                            : <Link href={hrefOfPage(shownPage)} aria-current={shownPage === page ? 'page' : undefined} className={buttonVariants({ variant: shownPage === page ? 'outline' : 'ghost', size: 'icon' })}>{shownPage}</Link>}
                    </PaginationItem>
                ))}
                <PaginationItem>
                    {page < pageCount
                        ? <Link href={hrefOfPage(page + 1)} aria-label="Go to next page" className={edgeClassName}><span className="hidden sm:block">Next</span><ChevronRight className="size-4" /></Link>
                        : <span aria-disabled="true" className={cn(edgeClassName, 'pointer-events-none opacity-50')}><span className="hidden sm:block">Next</span><ChevronRight className="size-4" /></span>}
                </PaginationItem>
            </PaginationContent>
        </Pagination>
    )
}
