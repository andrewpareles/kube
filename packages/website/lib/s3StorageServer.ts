import 'server-only'

import { CloudFrontClient, CreateInvalidationCommand } from '@aws-sdk/client-cloudfront'
import { DeleteObjectCommand, S3Client } from '@aws-sdk/client-s3'
import { keyOfPublicFile } from '@/lib/s3StorageFiles'

// The website's AWS user can only delete catalog files and clear CloudFront's copies of them.
const PUBLIC_BUCKET = 'kube-new-public'
const CLOUDFRONT_DISTRIBUTION_ID = 'E2MXAGMROR0BWM'

const getCredentials = () => {
    const accessKeyId = process.env.WEBSITE_S3_ACCESS_KEY_ID
    const secretAccessKey = process.env.WEBSITE_S3_SECRET_ACCESS_KEY
    if (!accessKeyId || !secretAccessKey) throw new Error('WEBSITE_S3_ACCESS_KEY_ID and WEBSITE_S3_SECRET_ACCESS_KEY must be set.')
    return { accessKeyId, secretAccessKey }
}

// A deleted catalog item's file is deleted and CloudFront forgets its copy, so nobody can load the item by its id again.
export const deleteCatalogFile = async (catalogItemId: number) => {
    const key = keyOfPublicFile({ kind: 'Catalog', catalogItemId })
    const credentials = getCredentials()
    await new S3Client({ region: 'us-east-1', credentials }).send(new DeleteObjectCommand({ Bucket: PUBLIC_BUCKET, Key: key }))
    await new CloudFrontClient({ region: 'us-east-1', credentials }).send(new CreateInvalidationCommand({
        DistributionId: CLOUDFRONT_DISTRIBUTION_ID,
        InvalidationBatch: { CallerReference: `${key}-${Date.now()}`, Paths: { Quantity: 1, Items: [`/${key}`] } },
    }))
}
