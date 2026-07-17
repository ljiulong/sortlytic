import {
  type QueryClient,
  useMutation,
  useQuery,
  useQueryClient,
} from '@tanstack/react-query'
import { useState } from 'react'
import {
  activateApiProfile,
  deleteApiProfile,
  getApiProfileRegistry,
  saveApiProfile,
  testApiProfile,
  type ApiProfileKind,
  type ApiProfileRegistryView,
  type ApiProfileTestResult,
  type SaveApiProfileInput,
} from './api-profiles'

export const API_PROFILE_REGISTRY_QUERY_KEY = ['api-profile-registry'] as const
export const WORKBENCH_BACKEND_QUERY_KEY = ['workbench-backend'] as const

type PrivateMutationOperation<T> = () => Promise<T>

export function useApiProfiles() {
  const queryClient = useQueryClient()
  const [isSaving, setIsSaving] = useState(false)
  const registryQuery = useQuery({
    queryKey: API_PROFILE_REGISTRY_QUERY_KEY,
    queryFn: getApiProfileRegistry,
    retry: 1,
  })
  const invalidateProfileConsumers = () => invalidateApiProfileQueries(queryClient)
  const testMutation = usePrivateApiProfileMutation<ApiProfileTestResult>(invalidateProfileConsumers)
  const activateMutation = usePrivateApiProfileMutation<ApiProfileRegistryView>(invalidateProfileConsumers)
  const deleteMutation = usePrivateApiProfileMutation<ApiProfileRegistryView>(invalidateProfileConsumers)

  const saveAndTestProfile = async (input: SaveApiProfileInput) => {
    setIsSaving(true)
    try {
      const registry = await saveApiProfile(input)
      const profileId = findSavedProfileId(registry, input)
      return await testApiProfile(input.kind, profileId)
    } finally {
      try {
        await invalidateProfileConsumers()
      } finally {
        setIsSaving(false)
      }
    }
  }

  const retestProfile = (kind: ApiProfileKind, profileId: string) => (
    testMutation.mutateAsync(() => testApiProfile(kind, profileId))
  )

  const activateProfile = (kind: ApiProfileKind, profileId: string) => (
    activateMutation.mutateAsync(() => activateApiProfile(kind, profileId))
  )

  const removeProfile = (kind: ApiProfileKind, profileId: string) => (
    deleteMutation.mutateAsync(() => deleteApiProfile(kind, profileId))
  )

  return {
    registryQuery,
    registry: registryQuery.data,
    saveAndTestProfile,
    retestProfile,
    activateProfile,
    deleteProfile: removeProfile,
    refreshProfiles: invalidateProfileConsumers,
    isSaving,
    isTesting: testMutation.isPending,
    isActivating: activateMutation.isPending,
    isDeleting: deleteMutation.isPending,
    isPending:
      isSaving
      || testMutation.isPending
      || activateMutation.isPending
      || deleteMutation.isPending,
  }
}

function invalidateApiProfileQueries(
  queryClient: Pick<QueryClient, 'invalidateQueries'>,
) {
  return Promise.all([
    queryClient.invalidateQueries({ queryKey: API_PROFILE_REGISTRY_QUERY_KEY }),
    queryClient.invalidateQueries({ queryKey: WORKBENCH_BACKEND_QUERY_KEY }),
  ])
}

function usePrivateApiProfileMutation<T>(
  invalidateRegistry: () => Promise<unknown>,
) {
  return useMutation<T, Error, PrivateMutationOperation<T>>({
    mutationFn: (operation) => operation(),
    onSettled: async () => {
      await invalidateRegistry()
    },
  })
}

function findSavedProfileId(
  registry: ApiProfileRegistryView,
  input: SaveApiProfileInput,
) {
  const profiles = input.kind === 'tikhub'
    ? registry.tikhubProfiles
    : registry.aiProfiles
  const requestedId = input.id?.trim()
  const savedProfile = requestedId
    ? profiles.find((profile) => profile.id === requestedId)
    : profiles.find((profile) => profile.name === input.name.trim())
  if (!savedProfile) {
    throw new Error('已保存 API 配置，但无法读取其安全视图，请刷新后重试')
  }
  return savedProfile.id
}
