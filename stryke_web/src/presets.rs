//! Hand-curated app presets for `s_web g app PRESET`.
//!
//! Each preset is a vector of `(ResourceName, &[field_specs])`. The
//! generator runs `cmd_generate::scaffold` against each entry, which
//! produces a model + migration + 7-action controller + 5 ERB views.
//! Field specs use the same `field:type` shape as `s_web g scaffold`.
//!
//! Naming policy: preset names are short, lowercase, and describe a
//! domain (`blog`, `ecommerce`, `saas`). The pseudo-preset `everything`
//! concatenates every other preset — one command, ~70 resources.

pub struct Resource {
    pub name: &'static str,
    pub fields: &'static [&'static str],
}

pub struct Preset {
    pub name: &'static str,
    pub description: &'static str,
    pub resources: &'static [Resource],
}

// ── Resource lists ──────────────────────────────────────────────────────
//
// Field types accepted: string, text, int, integer, bigint, float,
// decimal, bool, boolean, date, datetime, timestamp, references.
// `references` resolves to an integer FK column (no FK constraint
// auto-generated yet — that lands when migrations support it).

const BLOG: &[Resource] = &[
    Resource {
        name: "User",
        fields: &[
            "name:string",
            "email:string",
            "password_digest:string",
            "bio:text",
            "avatar:string",
        ],
    },
    Resource {
        name: "Post",
        fields: &[
            "title:string",
            "slug:string",
            "body:text",
            "published:bool",
            "user_id:int",
            "published_at:datetime",
        ],
    },
    Resource {
        name: "Comment",
        fields: &["body:text", "post_id:int", "user_id:int", "approved:bool"],
    },
    Resource {
        name: "Tag",
        fields: &["name:string", "slug:string"],
    },
    Resource {
        name: "Tagging",
        fields: &["post_id:int", "tag_id:int"],
    },
    Resource {
        name: "Category",
        fields: &["name:string", "slug:string", "parent_id:int"],
    },
    Resource {
        name: "Subscriber",
        fields: &["email:string", "confirmed:bool", "confirmed_at:datetime"],
    },
    Resource {
        name: "Pageview",
        fields: &[
            "post_id:int",
            "ip:string",
            "user_agent:string",
            "referrer:string",
        ],
    },
];

const ECOMMERCE: &[Resource] = &[
    Resource {
        name: "User",
        fields: &["name:string", "email:string", "password_digest:string"],
    },
    Resource {
        name: "Address",
        fields: &[
            "user_id:int",
            "line1:string",
            "line2:string",
            "city:string",
            "state:string",
            "zip:string",
            "country:string",
            "kind:string",
        ],
    },
    Resource {
        name: "Category",
        fields: &["name:string", "slug:string", "parent_id:int"],
    },
    Resource {
        name: "Product",
        fields: &[
            "name:string",
            "slug:string",
            "description:text",
            "price_cents:int",
            "sku:string",
            "stock:int",
            "active:bool",
            "category_id:int",
        ],
    },
    Resource {
        name: "Variant",
        fields: &[
            "product_id:int",
            "sku:string",
            "price_cents:int",
            "color:string",
            "size:string",
            "stock:int",
        ],
    },
    Resource {
        name: "Cart",
        fields: &["user_id:int", "session_id:string"],
    },
    Resource {
        name: "CartItem",
        fields: &[
            "cart_id:int",
            "product_id:int",
            "variant_id:int",
            "quantity:int",
        ],
    },
    Resource {
        name: "Order",
        fields: &[
            "user_id:int",
            "total_cents:int",
            "status:string",
            "placed_at:datetime",
            "shipping_address_id:int",
            "billing_address_id:int",
        ],
    },
    Resource {
        name: "OrderItem",
        fields: &[
            "order_id:int",
            "product_id:int",
            "variant_id:int",
            "quantity:int",
            "price_cents:int",
        ],
    },
    Resource {
        name: "Payment",
        fields: &[
            "order_id:int",
            "amount_cents:int",
            "method:string",
            "txn_id:string",
            "status:string",
        ],
    },
    Resource {
        name: "Shipment",
        fields: &[
            "order_id:int",
            "carrier:string",
            "tracking:string",
            "shipped_at:datetime",
            "delivered_at:datetime",
        ],
    },
    Resource {
        name: "Review",
        fields: &[
            "product_id:int",
            "user_id:int",
            "rating:int",
            "title:string",
            "body:text",
        ],
    },
    Resource {
        name: "Coupon",
        fields: &[
            "code:string",
            "discount_pct:int",
            "expires_at:datetime",
            "active:bool",
        ],
    },
    Resource {
        name: "WishlistItem",
        fields: &["user_id:int", "product_id:int"],
    },
    Resource {
        name: "Notification",
        fields: &[
            "user_id:int",
            "kind:string",
            "payload:text",
            "read:bool",
        ],
    },
];

const SAAS: &[Resource] = &[
    Resource {
        name: "User",
        fields: &["name:string", "email:string", "password_digest:string"],
    },
    Resource {
        name: "Organization",
        fields: &["name:string", "slug:string", "owner_id:int"],
    },
    Resource {
        name: "Membership",
        fields: &["organization_id:int", "user_id:int", "role:string"],
    },
    Resource {
        name: "Role",
        fields: &["name:string", "permissions:text"],
    },
    Resource {
        name: "Plan",
        fields: &[
            "name:string",
            "price_cents:int",
            "interval:string",
            "features:text",
            "active:bool",
        ],
    },
    Resource {
        name: "Subscription",
        fields: &[
            "organization_id:int",
            "plan_id:int",
            "status:string",
            "current_period_end:datetime",
        ],
    },
    Resource {
        name: "Invoice",
        fields: &[
            "subscription_id:int",
            "amount_cents:int",
            "status:string",
            "due_at:datetime",
            "paid_at:datetime",
        ],
    },
    Resource {
        name: "LineItem",
        fields: &[
            "invoice_id:int",
            "description:string",
            "amount_cents:int",
            "quantity:int",
        ],
    },
    Resource {
        name: "ApiKey",
        fields: &[
            "organization_id:int",
            "name:string",
            "key:string",
            "scopes:text",
            "last_used_at:datetime",
            "active:bool",
        ],
    },
    Resource {
        name: "AuditLog",
        fields: &[
            "organization_id:int",
            "user_id:int",
            "action:string",
            "payload:text",
            "ip:string",
        ],
    },
    Resource {
        name: "Webhook",
        fields: &[
            "organization_id:int",
            "url:string",
            "secret:string",
            "events:text",
            "active:bool",
        ],
    },
    Resource {
        name: "WebhookDelivery",
        fields: &[
            "webhook_id:int",
            "event:string",
            "payload:text",
            "status:string",
            "delivered_at:datetime",
        ],
    },
];

const SOCIAL: &[Resource] = &[
    Resource {
        name: "User",
        fields: &[
            "name:string",
            "username:string",
            "email:string",
            "password_digest:string",
            "bio:text",
            "avatar:string",
        ],
    },
    Resource {
        name: "Post",
        fields: &["user_id:int", "body:text", "image:string", "location:string"],
    },
    Resource {
        name: "Comment",
        fields: &["post_id:int", "user_id:int", "body:text"],
    },
    Resource {
        name: "Like",
        fields: &["user_id:int", "post_id:int"],
    },
    Resource {
        name: "Follow",
        fields: &["follower_id:int", "followed_id:int"],
    },
    Resource {
        name: "DirectMessage",
        fields: &[
            "sender_id:int",
            "receiver_id:int",
            "body:text",
            "read:bool",
        ],
    },
    Resource {
        name: "Notification",
        fields: &[
            "user_id:int",
            "kind:string",
            "payload:text",
            "read:bool",
        ],
    },
    Resource {
        name: "Hashtag",
        fields: &["name:string"],
    },
    Resource {
        name: "HashtagPost",
        fields: &["hashtag_id:int", "post_id:int"],
    },
    Resource {
        name: "Block",
        fields: &["blocker_id:int", "blocked_id:int", "reason:text"],
    },
];

const CMS: &[Resource] = &[
    Resource {
        name: "User",
        fields: &["name:string", "email:string", "password_digest:string"],
    },
    Resource {
        name: "Page",
        fields: &[
            "title:string",
            "slug:string",
            "body:text",
            "published:bool",
            "parent_id:int",
            "position:int",
        ],
    },
    Resource {
        name: "BlogPost",
        fields: &[
            "title:string",
            "slug:string",
            "body:text",
            "published:bool",
            "author_id:int",
            "published_at:datetime",
        ],
    },
    Resource {
        name: "Media",
        fields: &[
            "filename:string",
            "url:string",
            "kind:string",
            "size:int",
            "alt:string",
        ],
    },
    Resource {
        name: "Menu",
        fields: &["name:string", "location:string"],
    },
    Resource {
        name: "MenuItem",
        fields: &[
            "menu_id:int",
            "label:string",
            "url:string",
            "parent_id:int",
            "position:int",
        ],
    },
    Resource {
        name: "Form",
        fields: &["name:string", "slug:string", "fields:text"],
    },
    Resource {
        name: "FormSubmission",
        fields: &["form_id:int", "payload:text", "ip:string"],
    },
    Resource {
        name: "Setting",
        fields: &["key:string", "value:text"],
    },
    Resource {
        name: "RedirectRule",
        fields: &["source:string", "destination:string", "code:int"],
    },
    Resource {
        name: "Theme",
        fields: &["name:string", "slug:string", "active:bool"],
    },
    Resource {
        name: "Widget",
        fields: &[
            "name:string",
            "kind:string",
            "settings:text",
            "area:string",
            "position:int",
        ],
    },
];

const FORUM: &[Resource] = &[
    Resource {
        name: "User",
        fields: &["name:string", "email:string", "password_digest:string"],
    },
    Resource {
        name: "Category",
        fields: &[
            "name:string",
            "slug:string",
            "description:text",
            "position:int",
        ],
    },
    Resource {
        name: "Topic",
        fields: &[
            "category_id:int",
            "user_id:int",
            "title:string",
            "slug:string",
            "locked:bool",
            "sticky:bool",
        ],
    },
    Resource {
        name: "Post",
        fields: &["topic_id:int", "user_id:int", "body:text"],
    },
    Resource {
        name: "Reaction",
        fields: &["post_id:int", "user_id:int", "emoji:string"],
    },
    Resource {
        name: "Subscription",
        fields: &["user_id:int", "topic_id:int"],
    },
    Resource {
        name: "Badge",
        fields: &[
            "user_id:int",
            "kind:string",
            "awarded_at:datetime",
        ],
    },
    Resource {
        name: "Report",
        fields: &[
            "post_id:int",
            "user_id:int",
            "reason:text",
            "resolved:bool",
        ],
    },
    Resource {
        name: "Tag",
        fields: &["name:string", "slug:string"],
    },
    Resource {
        name: "TopicTag",
        fields: &["topic_id:int", "tag_id:int"],
    },
];

const CRM: &[Resource] = &[
    Resource {
        name: "User",
        fields: &["name:string", "email:string", "password_digest:string"],
    },
    Resource {
        name: "Account",
        fields: &[
            "name:string",
            "industry:string",
            "size:int",
            "website:string",
        ],
    },
    Resource {
        name: "Contact",
        fields: &[
            "account_id:int",
            "first_name:string",
            "last_name:string",
            "email:string",
            "phone:string",
            "title:string",
        ],
    },
    Resource {
        name: "Lead",
        fields: &[
            "contact_id:int",
            "source:string",
            "status:string",
            "value_cents:int",
            "owner_id:int",
        ],
    },
    Resource {
        name: "Opportunity",
        fields: &[
            "account_id:int",
            "name:string",
            "stage:string",
            "amount_cents:int",
            "close_date:date",
            "owner_id:int",
        ],
    },
    Resource {
        name: "Activity",
        fields: &[
            "contact_id:int",
            "kind:string",
            "body:text",
            "due_at:datetime",
            "completed:bool",
        ],
    },
    Resource {
        name: "Note",
        fields: &["contact_id:int", "user_id:int", "body:text"],
    },
    Resource {
        name: "Email",
        fields: &[
            "contact_id:int",
            "subject:string",
            "body:text",
            "sent_at:datetime",
        ],
    },
    Resource {
        name: "Task",
        fields: &[
            "assignee_id:int",
            "contact_id:int",
            "title:string",
            "due_at:datetime",
            "completed:bool",
        ],
    },
    Resource {
        name: "Pipeline",
        fields: &["name:string", "stages:text"],
    },
];

const AMAZON: &[Resource] = &[
    Resource {
        name: "User",
        fields: &[
            "name:string",
            "email:string",
            "password_digest:string",
            "phone:string",
            "prime_member:bool",
        ],
    },
    Resource {
        name: "Address",
        fields: &[
            "user_id:int",
            "name:string",
            "line1:string",
            "line2:string",
            "city:string",
            "state:string",
            "zip:string",
            "country:string",
            "kind:string",
            "is_default:bool",
        ],
    },
    Resource {
        name: "Department",
        fields: &["name:string", "slug:string", "icon:string", "position:int"],
    },
    Resource {
        name: "Category",
        fields: &[
            "department_id:int",
            "name:string",
            "slug:string",
            "parent_id:int",
        ],
    },
    Resource {
        name: "Brand",
        fields: &["name:string", "slug:string", "logo:string"],
    },
    Resource {
        name: "Product",
        fields: &[
            "name:string",
            "slug:string",
            "description:text",
            "brand_id:int",
            "category_id:int",
            "price_cents:int",
            "list_price_cents:int",
            "asin:string",
            "stock:int",
            "rating_avg:float",
            "rating_count:int",
            "active:bool",
            "is_prime:bool",
            "ship_weight_grams:int",
        ],
    },
    Resource {
        name: "Variant",
        fields: &[
            "product_id:int",
            "sku:string",
            "asin:string",
            "color:string",
            "size:string",
            "price_cents:int",
            "stock:int",
        ],
    },
    Resource {
        name: "ProductImage",
        fields: &[
            "product_id:int",
            "url:string",
            "alt:string",
            "position:int",
        ],
    },
    Resource {
        name: "Cart",
        fields: &["user_id:int", "session_id:string"],
    },
    Resource {
        name: "CartItem",
        fields: &[
            "cart_id:int",
            "product_id:int",
            "variant_id:int",
            "quantity:int",
            "saved_for_later:bool",
        ],
    },
    Resource {
        name: "Order",
        fields: &[
            "user_id:int",
            "subtotal_cents:int",
            "shipping_cents:int",
            "tax_cents:int",
            "total_cents:int",
            "status:string",
            "placed_at:datetime",
            "shipping_address_id:int",
            "billing_address_id:int",
            "tracking_number:string",
        ],
    },
    Resource {
        name: "OrderItem",
        fields: &[
            "order_id:int",
            "product_id:int",
            "variant_id:int",
            "quantity:int",
            "price_cents:int",
            "title_at_purchase:string",
        ],
    },
    Resource {
        name: "Payment",
        fields: &[
            "order_id:int",
            "amount_cents:int",
            "method:string",
            "card_last4:string",
            "txn_id:string",
            "status:string",
        ],
    },
    Resource {
        name: "Shipment",
        fields: &[
            "order_id:int",
            "carrier:string",
            "tracking:string",
            "shipped_at:datetime",
            "delivered_at:datetime",
            "status:string",
        ],
    },
    Resource {
        name: "Review",
        fields: &[
            "product_id:int",
            "user_id:int",
            "order_id:int",
            "rating:int",
            "title:string",
            "body:text",
            "verified_purchase:bool",
            "helpful_count:int",
        ],
    },
    Resource {
        name: "Question",
        fields: &[
            "product_id:int",
            "user_id:int",
            "body:text",
            "answered:bool",
        ],
    },
    Resource {
        name: "Answer",
        fields: &[
            "question_id:int",
            "user_id:int",
            "body:text",
            "helpful_count:int",
        ],
    },
    Resource {
        name: "Wishlist",
        fields: &["user_id:int", "name:string", "is_public:bool"],
    },
    Resource {
        name: "WishlistItem",
        fields: &["wishlist_id:int", "product_id:int", "note:text"],
    },
    Resource {
        name: "Recommendation",
        fields: &[
            "user_id:int",
            "product_id:int",
            "score:float",
            "reason:string",
        ],
    },
    Resource {
        name: "Coupon",
        fields: &[
            "code:string",
            "discount_pct:int",
            "discount_cents:int",
            "expires_at:datetime",
            "active:bool",
            "min_order_cents:int",
        ],
    },
    Resource {
        name: "Notification",
        fields: &[
            "user_id:int",
            "kind:string",
            "payload:text",
            "read:bool",
        ],
    },
    Resource {
        name: "Seller",
        fields: &[
            "name:string",
            "slug:string",
            "rating_avg:float",
            "rating_count:int",
            "is_amazon:bool",
        ],
    },
    Resource {
        name: "Listing",
        fields: &[
            "seller_id:int",
            "product_id:int",
            "variant_id:int",
            "price_cents:int",
            "stock:int",
            "condition:string",
        ],
    },
    Resource {
        name: "Return",
        fields: &[
            "order_id:int",
            "user_id:int",
            "reason:string",
            "status:string",
            "refund_cents:int",
            "filed_at:datetime",
        ],
    },
];

const FACEBOOK: &[Resource] = &[
    Resource {
        name: "User",
        fields: &[
            "name:string",
            "username:string",
            "email:string",
            "password_digest:string",
            "bio:text",
            "avatar:string",
            "cover_photo:string",
            "birthday:date",
            "gender:string",
            "city:string",
            "country:string",
            "verified:bool",
        ],
    },
    Resource {
        name: "Friendship",
        fields: &[
            "requester_id:int",
            "addressee_id:int",
            "status:string",
            "accepted_at:datetime",
        ],
    },
    Resource {
        name: "Post",
        fields: &[
            "user_id:int",
            "body:text",
            "image:string",
            "video:string",
            "location:string",
            "feeling:string",
            "audience:string",
        ],
    },
    Resource {
        name: "PostShare",
        fields: &[
            "post_id:int",
            "user_id:int",
            "comment:text",
        ],
    },
    Resource {
        name: "Comment",
        fields: &[
            "post_id:int",
            "user_id:int",
            "parent_id:int",
            "body:text",
        ],
    },
    Resource {
        name: "Reaction",
        fields: &[
            "user_id:int",
            "target_id:int",
            "target_type:string",
            "kind:string",
        ],
    },
    Resource {
        name: "Photo",
        fields: &[
            "user_id:int",
            "album_id:int",
            "url:string",
            "caption:text",
            "taken_at:datetime",
        ],
    },
    Resource {
        name: "Album",
        fields: &[
            "user_id:int",
            "name:string",
            "description:text",
            "cover_photo_id:int",
        ],
    },
    Resource {
        name: "Group",
        fields: &[
            "name:string",
            "slug:string",
            "description:text",
            "cover_photo:string",
            "privacy:string",
            "owner_id:int",
        ],
    },
    Resource {
        name: "GroupMembership",
        fields: &[
            "group_id:int",
            "user_id:int",
            "role:string",
            "joined_at:datetime",
        ],
    },
    Resource {
        name: "Event",
        fields: &[
            "creator_id:int",
            "name:string",
            "description:text",
            "starts_at:datetime",
            "ends_at:datetime",
            "location:string",
            "cover_photo:string",
        ],
    },
    Resource {
        name: "EventRsvp",
        fields: &["event_id:int", "user_id:int", "status:string"],
    },
    Resource {
        name: "DirectMessage",
        fields: &[
            "sender_id:int",
            "receiver_id:int",
            "body:text",
            "read:bool",
            "sent_at:datetime",
        ],
    },
    Resource {
        name: "Conversation",
        fields: &["title:string", "is_group:bool"],
    },
    Resource {
        name: "ConversationMember",
        fields: &["conversation_id:int", "user_id:int"],
    },
    Resource {
        name: "Notification",
        fields: &[
            "user_id:int",
            "kind:string",
            "actor_id:int",
            "target_id:int",
            "target_type:string",
            "read:bool",
        ],
    },
    Resource {
        name: "Page",
        fields: &[
            "owner_id:int",
            "name:string",
            "slug:string",
            "category:string",
            "description:text",
            "verified:bool",
        ],
    },
    Resource {
        name: "PageFollow",
        fields: &["page_id:int", "user_id:int"],
    },
    Resource {
        name: "Story",
        fields: &[
            "user_id:int",
            "image:string",
            "video:string",
            "expires_at:datetime",
        ],
    },
    Resource {
        name: "Block",
        fields: &["blocker_id:int", "blocked_id:int"],
    },
    Resource {
        name: "Hashtag",
        fields: &["name:string"],
    },
    Resource {
        name: "HashtagPost",
        fields: &["hashtag_id:int", "post_id:int"],
    },
    Resource {
        name: "Mention",
        fields: &[
            "user_id:int",
            "post_id:int",
            "comment_id:int",
        ],
    },
];

const LEARNING: &[Resource] = &[
    Resource {
        name: "User",
        fields: &[
            "name:string",
            "email:string",
            "password_digest:string",
            "study_streak_days:int",
            "timezone:string",
        ],
    },
    Resource {
        name: "Course",
        fields: &[
            "user_id:int",
            "name:string",
            "slug:string",
            "description:text",
            "subject:string",
            "color:string",
            "is_archived:bool",
        ],
    },
    Resource {
        name: "Lesson",
        fields: &[
            "course_id:int",
            "title:string",
            "body:text",
            "position:int",
            "duration_minutes:int",
            "completed_at:datetime",
        ],
    },
    Resource {
        name: "Note",
        fields: &[
            "user_id:int",
            "course_id:int",
            "lesson_id:int",
            "title:string",
            "body:text",
        ],
    },
    Resource {
        name: "Deck",
        fields: &[
            "user_id:int",
            "course_id:int",
            "name:string",
            "slug:string",
            "description:text",
        ],
    },
    Resource {
        name: "Flashcard",
        fields: &[
            "deck_id:int",
            "front:text",
            "back:text",
            "hint:text",
            "tags:string",
        ],
    },
    Resource {
        name: "Review",
        fields: &[
            "flashcard_id:int",
            "user_id:int",
            "ease:float",
            "interval_days:int",
            "next_review_at:datetime",
            "rating:int",
        ],
    },
    Resource {
        name: "StudySession",
        fields: &[
            "user_id:int",
            "course_id:int",
            "started_at:datetime",
            "ended_at:datetime",
            "cards_reviewed:int",
            "minutes:int",
        ],
    },
    Resource {
        name: "Goal",
        fields: &[
            "user_id:int",
            "course_id:int",
            "title:string",
            "target_minutes:int",
            "deadline:date",
            "completed_at:datetime",
        ],
    },
    Resource {
        name: "Resource",
        fields: &[
            "user_id:int",
            "course_id:int",
            "kind:string",
            "title:string",
            "url:string",
            "notes:text",
        ],
    },
    Resource {
        name: "Tag",
        fields: &["user_id:int", "name:string", "color:string"],
    },
    Resource {
        name: "Tagging",
        fields: &[
            "tag_id:int",
            "taggable_id:int",
            "taggable_type:string",
        ],
    },
    Resource {
        name: "Quiz",
        fields: &[
            "course_id:int",
            "title:string",
            "description:text",
            "duration_minutes:int",
        ],
    },
    Resource {
        name: "QuizQuestion",
        fields: &[
            "quiz_id:int",
            "kind:string",
            "prompt:text",
            "explanation:text",
            "position:int",
        ],
    },
    Resource {
        name: "QuizChoice",
        fields: &[
            "quiz_question_id:int",
            "body:text",
            "is_correct:bool",
        ],
    },
    Resource {
        name: "QuizAttempt",
        fields: &[
            "quiz_id:int",
            "user_id:int",
            "score_pct:float",
            "started_at:datetime",
            "finished_at:datetime",
        ],
    },
    Resource {
        name: "Progress",
        fields: &[
            "user_id:int",
            "course_id:int",
            "completed_lessons:int",
            "total_lessons:int",
            "last_studied_at:datetime",
        ],
    },
    Resource {
        name: "Streak",
        fields: &[
            "user_id:int",
            "current_days:int",
            "longest_days:int",
            "last_active_on:date",
        ],
    },
    Resource {
        name: "Achievement",
        fields: &[
            "user_id:int",
            "kind:string",
            "name:string",
            "icon:string",
            "earned_at:datetime",
        ],
    },
    Resource {
        name: "Bookmark",
        fields: &[
            "user_id:int",
            "url:string",
            "title:string",
            "notes:text",
            "course_id:int",
        ],
    },
    Resource {
        name: "Highlight",
        fields: &[
            "user_id:int",
            "resource_id:int",
            "lesson_id:int",
            "body:text",
            "color:string",
        ],
    },
];

const HELPDESK: &[Resource] = &[
    Resource {
        name: "User",
        fields: &["name:string", "email:string", "password_digest:string"],
    },
    Resource {
        name: "Customer",
        fields: &["name:string", "email:string", "phone:string", "tier:string"],
    },
    Resource {
        name: "Ticket",
        fields: &[
            "customer_id:int",
            "assignee_id:int",
            "subject:string",
            "body:text",
            "status:string",
            "priority:string",
        ],
    },
    Resource {
        name: "Reply",
        fields: &[
            "ticket_id:int",
            "user_id:int",
            "body:text",
            "internal:bool",
        ],
    },
    Resource {
        name: "KnowledgeArticle",
        fields: &[
            "title:string",
            "slug:string",
            "body:text",
            "category_id:int",
            "published:bool",
        ],
    },
    Resource {
        name: "Sla",
        fields: &[
            "name:string",
            "first_response_minutes:int",
            "resolution_minutes:int",
        ],
    },
    Resource {
        name: "Tag",
        fields: &["name:string"],
    },
    Resource {
        name: "TicketTag",
        fields: &["ticket_id:int", "tag_id:int"],
    },
];

// ── Registry ────────────────────────────────────────────────────────────

pub const PRESETS: &[Preset] = &[
    Preset {
        name: "blog",
        description: "Posts, comments, tags, categories, subscribers, pageviews.",
        resources: BLOG,
    },
    Preset {
        name: "ecommerce",
        description: "Products, variants, carts, orders, payments, shipments, reviews, coupons.",
        resources: ECOMMERCE,
    },
    Preset {
        name: "saas",
        description: "Orgs, memberships, plans, subscriptions, invoices, api keys, audit logs, webhooks.",
        resources: SAAS,
    },
    Preset {
        name: "social",
        description: "Posts, comments, likes, follows, DMs, notifications, hashtags, blocks.",
        resources: SOCIAL,
    },
    Preset {
        name: "cms",
        description: "Pages, blog posts, media, menus, forms, settings, redirects, themes, widgets.",
        resources: CMS,
    },
    Preset {
        name: "forum",
        description: "Categories, topics, posts, reactions, subscriptions, badges, reports, tags.",
        resources: FORUM,
    },
    Preset {
        name: "crm",
        description: "Accounts, contacts, leads, opportunities, activities, notes, emails, tasks, pipelines.",
        resources: CRM,
    },
    Preset {
        name: "helpdesk",
        description: "Customers, tickets, replies, knowledge base, SLAs, tags.",
        resources: HELPDESK,
    },
    Preset {
        name: "amazon",
        description: "Amazon-clone marketplace: departments, brands, products, variants, carts, orders, payments, shipments, reviews, Q&A, wishlists, recommendations, sellers, listings, returns.",
        resources: AMAZON,
    },
    Preset {
        name: "facebook",
        description: "Facebook-clone social: friendships, posts, comments, reactions, photos, albums, groups, events, RSVPs, DMs, stories, pages, hashtags, mentions.",
        resources: FACEBOOK,
    },
    Preset {
        name: "learning",
        description: "Anki-style learning tracker: courses, lessons, notes, decks, flashcards, spaced-repetition reviews, study sessions, goals, quizzes, achievements, streaks.",
        resources: LEARNING,
    },
];

pub fn lookup(name: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.name == name)
}

/// `everything` is the union of all real presets — one command,
/// ~80 resources, every CRUD route a person could possibly want
/// pre-wired. Useful for stress-testing the framework or as a
/// sandbox to delete-down from.
pub fn everything_resources() -> Vec<&'static Resource> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for preset in PRESETS {
        for r in preset.resources {
            if seen.insert(r.name) {
                out.push(r);
            }
        }
    }
    out
}
