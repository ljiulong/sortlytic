export const accountFieldGroups = [
  {
    key: 'profile',
    fields: [
      'secure_user_id', 'avatar_url', 'profile_url', 'bio', 'website_url',
      'verification_status', 'verification_reason', 'account_type', 'private_account',
      'language', 'country_region', 'profile_tags',
    ],
  },
  { key: 'demographics', fields: ['gender', 'age'] },
  {
    key: 'statistics',
    fields: [
      'followers_count', 'following_count', 'friends_count', 'posts_count',
      'likes_received_count', 'liked_content_count',
    ],
  },
  {
    key: 'activity',
    fields: [
      'account_created_at', 'last_posted_at', 'live_status', 'live_room_id',
      'username_modified_at', 'nickname_modified_at',
    ],
  },
  {
    key: 'platform_specific',
    fields: [
      'commerce_status', 'commerce_category', 'seller_status', 'organization_status',
      'comments_permission', 'duet_permission', 'stitch_permission', 'download_permission',
      'favorites_visibility', 'following_visibility', 'playlist_visibility', 'live_level',
      'live_badge',
    ],
  },
] as const

export const catalogAccountFields: ReadonlySet<string> = new Set(
  accountFieldGroups.flatMap((group) => group.fields),
)
