import { parse } from 'dotenv'
import fs from 'node:fs'

export const readEnvVar = ({ envFilePath, envKey, }: { envFilePath: string, envKey: string }): string => {
  const envOfEnvFile = fs.existsSync(envFilePath) ? parse(fs.readFileSync(envFilePath)) : {}
  const envValue = envOfEnvFile[envKey]
  if (!envValue) {
    throw new Error(`Missing ${envKey} in ${envFilePath}`)
  }
  return envValue
}
