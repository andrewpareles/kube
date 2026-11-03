import { getSiteUrl } from './siteMetadata.ts'

// The names of the pages that other pages sit under. Every page states its own name, so this table
// only holds the two paths that show up as somebody else's parent.
const breadcrumbNameOfAncestorPath: { [path: string]: string } = {
    '/': 'Home',
    '/blog': 'Blog',
}

// Breadcrumbs follow the URL hierarchy. Paths missing from the table above are left out, which keeps
// dynamic routes such as /profile/123 from linking search engines to the signed-in /profile route.
export const getBreadcrumbJsonLd = ({ path, pageName }: { path: string; pageName: string }) => {
    const breadcrumbItems = [{ name: breadcrumbNameOfAncestorPath['/'], path: '/' }]
    const pathSegments = path.split('/').filter((pathSegment) => pathSegment !== '')
    let breadcrumbPath = ''
    for (const pathSegment of pathSegments) {
        breadcrumbPath += `/${pathSegment}`
        const isCurrentPage = breadcrumbPath === path
        const breadcrumbName = isCurrentPage ? pageName : breadcrumbNameOfAncestorPath[breadcrumbPath]
        if (breadcrumbName === undefined) continue
        breadcrumbItems.push({
            name: breadcrumbName,
            path: breadcrumbPath,
        })
    }

    return {
        '@context': 'https://schema.org',
        '@type': 'BreadcrumbList',
        itemListElement: breadcrumbItems.map((breadcrumbItem, index) => ({
            '@type': 'ListItem',
            position: index + 1,
            name: breadcrumbItem.name,
            item: getSiteUrl(breadcrumbItem.path),
        })),
    }
}

// Escaping less-than signs keeps user-written names and descriptions from ending the script element early.
export const getJsonLdScriptText = (value: object) => JSON.stringify(value).replace(/</g, '\\u003c')
