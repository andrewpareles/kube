'use client'

import { signIn } from "@/app/server-actions/sign-in"
import { createAccount } from "@/app/server-actions/sign-up"
import { getAuthErrorMessage } from "@/lib/siteAuthErrors"
import { supabaseOnClient } from "@/supabase/supabaseOnClient"
import { redirect } from "next/navigation"
import { useState } from "react"

export const useEmailLoginState = (formRef: React.RefObject<HTMLFormElement | null>) => {
  const [loginState, setLoginState] = useState<
    | {
      type: "idle"
      result: { type: "error"; error: string } | null
    }
    | {
      type: "pending"
    }
  >({ type: "idle", result: null })

  // Signing in happens on the server, which redirects to next when it works, so the only answer that
  // ever comes back here is an error.
  const login = async ({ next }: { next: string }) => {
    if (!formRef.current) return
    const formData = new FormData(formRef.current)
    setLoginState({ type: "pending" })
    const email = formData.get("email") as string
    const password = formData.get("password") as string
    const { error } = await signIn({ email, password, next })
    setLoginState({ type: "idle", result: { type: "error", error } })
  }

  return { loginState, login }
}

export const useEmailSignupState = (formRef: React.RefObject<HTMLFormElement | null>) => {
  const [signupState, setSignupState] = useState<
    | {
      type: "idle"
      result:
      | { type: "error"; error: string }
      | { type: "checkEmail" }
      | null
    }
    | {
      type: "pending"
    }
  >({ type: "idle", result: null })

  const signup = async ({ emailRedirectToOnSuccess }: { emailRedirectToOnSuccess: string }) => {
    if (!formRef.current) return
    const formData = new FormData(formRef.current)
    setSignupState({ type: "pending" })
    const email = formData.get("email") as string
    const password = formData.get("password") as string
    // Signing up is the one of these three that goes through the server, because the password length
    // has to be checked somewhere the page cannot skip.
    const result = await createAccount({ email, password, emailRedirectTo: emailRedirectToOnSuccess })
    if (result.type === "error") {
      setSignupState({
        type: "idle",
        result: { type: "error", error: result.error },
      })
      return false
    } else {
      setSignupState({ type: "idle", result: { type: "checkEmail" } })
      return true
    }
  }

  return { signupState, signup }
}



export const useResetPasswordState = (formRef: React.RefObject<HTMLFormElement | null>) => {
  const [resetPasswordState, setResetPasswordState] = useState<
    | {
      type: "idle"
      result:
      | { type: "error"; error: string }
      | { type: "checkEmail" }
      | null
    }
    | {
      type: "pending"
    }
  >({ type: "idle", result: null })

  const resetPassword = async ({ emailRedirectToOnSuccess, }: { emailRedirectToOnSuccess: string }) => {
    if (!formRef.current) return
    const formData = new FormData(formRef.current)
    const email = formData.get("email") as string
    setResetPasswordState({ type: "pending" })
    const supabase = supabaseOnClient()
    const { error } = await supabase.auth.resetPasswordForEmail(email, {
      redirectTo: emailRedirectToOnSuccess,
    })
    if (error) {
      setResetPasswordState({
        type: "idle",
        result: { type: "error", error: getAuthErrorMessage({ error }) },
      })
      return false
    } else {
      setResetPasswordState({ type: "idle", result: { type: "checkEmail" } })
      return true
    }
  }

  return { resetPasswordState, resetPassword }
}


export const useNewPasswordState = (formRef: React.RefObject<HTMLFormElement | null>) => {
  const [newPasswordState, setNewPasswordState] = useState<
    | {
      type: "idle"
      result: { type: "error"; error: string } | null
    }
    | {
      type: "pending"
    }
  >({ type: "idle", result: null })

  // The password is changed on whoever the session says is signed in, which is why this only works
  // on the page a reset link lands on: following that link is what signs them in to begin with.
  const setNewPassword = async ({ redirectToOnSuccess }: { redirectToOnSuccess: string }) => {
    if (!formRef.current) return
    const formData = new FormData(formRef.current)
    const password = formData.get("password") as string
    setNewPasswordState({ type: "pending" })
    const supabase = supabaseOnClient()
    const { error } = await supabase.auth.updateUser({ password })
    if (error) {
      setNewPasswordState({
        type: "idle",
        result: { type: "error", error: getAuthErrorMessage({ error }) },
      })
      return false
    }
    setNewPasswordState({ type: "idle", result: null })
    redirect(redirectToOnSuccess)
  }

  return { newPasswordState, setNewPassword }
}
