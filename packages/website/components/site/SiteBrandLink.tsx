import Link from 'next/link'
import { KubeLogoMark } from '@/components/site/KubeLogoMark'
import { foldHoverCycle, restingKView } from '@/lib/kubeLogo/kubeLogoRender'
import { CREATE_PATH } from '@/lib/siteNavigation'

export const SiteBrandLink = ({ renderId, logoSidePx }: { renderId: string, logoSidePx: number }) => (
    <Link href={CREATE_PATH}className="site-brand" data-kube-logo-hover="" aria-label="kube.new home">
        <KubeLogoMark
            view={restingKView}
            hoverCycle={foldHoverCycle}
            sidePx={logoSidePx}
            hoverAreaOverhangPx={0}
            strokeScale={1.51}
            renderId={renderId}
            label={null}
            introMilliseconds={null}
            isKLevel={true}
        />
        <span className="site-wordmark">kube<span className="site-wordmark-suffix">.new</span></span>
    </Link>
)
