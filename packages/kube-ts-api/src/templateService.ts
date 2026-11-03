import { denoOps } from "./object_types/__globalOps__"
import type { ObjectId } from "./object_types/shared"
import { getRustObject, type LiveObject } from "./world"

/** This service exposes methods for storing and creating object templates. */
export type TemplateService = {
  cutObjectToTemplate: (objectId: ObjectId, templateName: string) => void
  pasteTemplate: (templateName: string, parentId: ObjectId) => LiveObject
}

export const templateService: TemplateService = {
  cutObjectToTemplate: (objectId: ObjectId, templateName: string) => {
    denoOps.__cutRustObjToTemplate__(objectId, templateName)
  },
  pasteTemplate: (templateName: string, parentId: ObjectId) => {
    const pastedObjectId = denoOps.__pasteRustObjFromTemplate__(templateName, parentId)
    const pastedObject = getRustObject(pastedObjectId)
    if (pastedObject === null) {
      throw new Error(`Pasted object with id ${pastedObjectId} was not found`)
    }
    return pastedObject
  },
}
