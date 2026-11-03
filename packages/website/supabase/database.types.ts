export type Json =
  | string
  | number
  | boolean
  | null
  | { [key: string]: Json | undefined }
  | Json[]

export type Database = {
  // Allows to automatically instantiate createClient with right options
  // instead of createClient<Database, { PostgrestVersion: 'XX' }>(URL, KEY)
  __InternalSupabase: {
    PostgrestVersion: "14.1"
  }
  public: {
    Tables: {
      editor_share_auth: {
        Row: {
          game_id: number
          invite_token: string | null
        }
        Insert: {
          game_id: number
          invite_token?: string | null
        }
        Update: {
          game_id?: number
          invite_token?: string | null
        }
        Relationships: [
          {
            foreignKeyName: "editor_share_auth_game_id_fkey"
            columns: ["game_id"]
            isOneToOne: true
            referencedRelation: "game_metadata"
            referencedColumns: ["game_id"]
          },
        ]
      }
      game_editor_worlds: {
        Row: {
          game_id: number
          updated_at: string
        }
        Insert: {
          game_id: number
          updated_at?: string
        }
        Update: {
          game_id?: number
          updated_at?: string
        }
        Relationships: [
          {
            foreignKeyName: "game_editor_worlds_game_id_fkey"
            columns: ["game_id"]
            isOneToOne: true
            referencedRelation: "game_metadata"
            referencedColumns: ["game_id"]
          },
        ]
      }
      starting_editor_servers: {
        Row: {
          request_id: string
          game_id: number
          starting_at: string
        }
        Insert: {
          request_id: string
          game_id: number
          starting_at?: string
        }
        Update: {
          request_id?: string
          game_id?: number
          starting_at?: string
        }
        Relationships: [
          {
            foreignKeyName: "starting_editor_servers_game_id_fkey"
            columns: ["game_id"]
            isOneToOne: true
            referencedRelation: "game_metadata"
            referencedColumns: ["game_id"]
          },
        ]
      }
      running_editor_servers: {
        Row: {
          request_id: string
          game_id: number
          editor_url: string
          public_ip: string
          playtest_external_port: number
        }
        Insert: {
          request_id: string
          game_id: number
          editor_url: string
          public_ip: string
          playtest_external_port: number
        }
        Update: {
          request_id?: string
          game_id?: number
          editor_url?: string
          public_ip?: string
          playtest_external_port?: number
        }
        Relationships: [
          {
            foreignKeyName: "running_editor_servers_game_id_fkey"
            columns: ["game_id"]
            isOneToOne: true
            referencedRelation: "game_metadata"
            referencedColumns: ["game_id"]
          },
        ]
      }
      chat_messages: {
        Row: {
          game_id: number
          seq: number
          message: Json
          created_at: string
        }
        Insert: {
          game_id: number
          seq: number
          message: Json
          created_at?: string
        }
        Update: {
          game_id?: number
          seq?: number
          message?: Json
          created_at?: string
        }
        Relationships: [
          {
            foreignKeyName: "chat_messages_game_id_fkey"
            columns: ["game_id"]
            isOneToOne: false
            referencedRelation: "game_metadata"
            referencedColumns: ["game_id"]
          },
        ]
      }
      chat_usage: {
        Row: {
          id: number
          profile_id: string
          game_id: number | null
          model_id: string
          input_tokens: number
          cache_write_tokens: number
          cache_read_tokens: number
          output_tokens: number | null
          aws_request_id: string
          created_at: string
          credits_charged: number
          message_id: string | null
        }
        Insert: {
          id?: never
          profile_id: string
          game_id?: number | null
          model_id: string
          input_tokens?: number
          cache_write_tokens?: number
          cache_read_tokens?: number
          output_tokens?: number | null
          aws_request_id: string
          created_at?: string
          credits_charged: number
          message_id?: string | null
        }
        Update: {
          id?: never
          profile_id?: string
          game_id?: number | null
          model_id?: string
          input_tokens?: number
          cache_write_tokens?: number
          cache_read_tokens?: number
          output_tokens?: number | null
          aws_request_id?: string
          created_at?: string
          credits_charged?: number
          message_id?: string | null
        }
        Relationships: [
          {
            foreignKeyName: "chat_usage_game_id_fkey"
            columns: ["game_id"]
            isOneToOne: false
            referencedRelation: "game_metadata"
            referencedColumns: ["game_id"]
          },
          {
            foreignKeyName: "chat_usage_profile_id_fkey"
            columns: ["profile_id"]
            isOneToOne: false
            referencedRelation: "user_profiles"
            referencedColumns: ["id"]
          },
        ]
      }
      blender_metadata: {
        Row: {
          blender_id: number
          created_at: string
          source_format: string
          uploaded_by: string
        }
        Insert: {
          blender_id?: number
          created_at?: string
          source_format?: string
          uploaded_by: string
        }
        Update: {
          blender_id?: number
          created_at?: string
          source_format?: string
          uploaded_by?: string
        }
        Relationships: [
          {
            foreignKeyName: "blender_metadata_uploaded_by_fkey"
            columns: ["uploaded_by"]
            isOneToOne: false
            referencedRelation: "user_profiles"
            referencedColumns: ["id"]
          },
        ]
      }
      data_metadata: {
        Row: {
          created_at: string
          data_id: number
          data_type: Database["public"]["Enums"]["data_type"]
          id: string
          uploaded_by: string
        }
        Insert: {
          created_at?: string
          data_id: number
          data_type: Database["public"]["Enums"]["data_type"]
          id?: string
          uploaded_by: string
        }
        Update: {
          created_at?: string
          data_id?: number
          data_type?: Database["public"]["Enums"]["data_type"]
          id?: string
          uploaded_by?: string
        }
        Relationships: [
          {
            foreignKeyName: "data_metadata_uploaded_by_fkey"
            columns: ["uploaded_by"]
            isOneToOne: false
            referencedRelation: "user_profiles"
            referencedColumns: ["id"]
          },
        ]
      }
      asset_metadata: {
        Row: {
          asset_id: number
          version_num: number
          name: string
          created_at: string
          uploaded_by: string
          asset_type: Database["public"]["Enums"]["asset_type"]
          import_count: number
        }
        Insert: {
          asset_id?: number
          version_num: number
          name: string
          created_at?: string
          uploaded_by: string
          asset_type: Database["public"]["Enums"]["asset_type"]
          import_count?: number
        }
        Update: {
          asset_id?: number
          version_num?: number
          name?: string
          created_at?: string
          uploaded_by?: string
          asset_type?: Database["public"]["Enums"]["asset_type"]
          import_count?: number
        }
        Relationships: [{
          foreignKeyName: "asset_metadata_uploaded_by_fkey"
          columns: ["uploaded_by"]
          isOneToOne: false
          referencedRelation: "user_profiles"
          referencedColumns: ["id"]
        }]
      }
      catalog_metadata: {
        Row: {
          catalog_item_id: number
          creator_id: string
          name: string
          description: string | null
          is_listed: boolean
          created_at: string
          updated_at: string
        }
        Insert: {
          catalog_item_id?: number
          creator_id: string
          name: string
          description?: string | null
          is_listed?: boolean
          created_at?: string
          updated_at?: string
        }
        Update: {
          catalog_item_id?: number
          creator_id?: string
          name?: string
          description?: string | null
          is_listed?: boolean
          created_at?: string
          updated_at?: string
        }
        Relationships: [{
          foreignKeyName: "catalog_metadata_creator_id_fkey"
          columns: ["creator_id"]
          isOneToOne: false
          referencedRelation: "user_profiles"
          referencedColumns: ["id"]
        }]
      }
      user_owned_catalog_items: {
        Row: {
          profile_id: string
          catalog_item_id: number
          purchased_at: string
        }
        Insert: {
          profile_id: string
          catalog_item_id: number
          purchased_at?: string
        }
        Update: {
          profile_id?: string
          catalog_item_id?: number
          purchased_at?: string
        }
        Relationships: [{
          foreignKeyName: "user_owned_catalog_items_profile_id_fkey"
          columns: ["profile_id"]
          isOneToOne: false
          referencedRelation: "user_profiles"
          referencedColumns: ["id"]
        }, {
          foreignKeyName: "user_owned_catalog_items_catalog_item_id_fkey"
          columns: ["catalog_item_id"]
          isOneToOne: false
          referencedRelation: "catalog_metadata"
          referencedColumns: ["catalog_item_id"]
        }]
      }
      game_metadata: {
        Row: {
          created_at: string | null
          creator_id: string
          description: string | null
          game_id: number
          is_published: boolean | null
          max_players: number
          name: string
          total_visits: number
        }
        Insert: {
          created_at?: string | null
          creator_id: string
          description?: string | null
          game_id?: number
          is_published?: boolean | null
          max_players: number
          name: string
          total_visits?: number
        }
        Update: {
          created_at?: string | null
          creator_id?: string
          description?: string | null
          game_id?: number
          is_published?: boolean | null
          max_players?: number
          name?: string
          total_visits?: number
        }
        Relationships: [
          {
            foreignKeyName: "game_metadata_creator_id_fkey"
            columns: ["creator_id"]
            isOneToOne: false
            referencedRelation: "user_profiles"
            referencedColumns: ["id"]
          },
        ]
      }
      game_worlds: {
        Row: {
          created_at: string | null
          game_id: number
          world_id: string
          version_num: number
        }
        Insert: {
          created_at?: string | null
          game_id: number
          world_id: string
          version_num?: number
        }
        Update: {
          created_at?: string | null
          game_id?: number
          world_id?: string
          version_num?: number
        }
        Relationships: []
      }
      user_desktop_play_auth: {
        Row: {
          play_auth_hash: string | null
          profile_id: string
        }
        Insert: {
          play_auth_hash?: string | null
          profile_id: string
        }
        Update: {
          play_auth_hash?: string | null
          profile_id?: string
        }
        Relationships: [
          {
            foreignKeyName: "player_auth_user_profile_id_fkey"
            columns: ["profile_id"]
            isOneToOne: true
            referencedRelation: "user_profiles"
            referencedColumns: ["id"]
          },
        ]
      }
      running_player_sessions: {
        Row: {
          profile_id: string
          request_id: string
        }
        Insert: {
          profile_id: string
          request_id: string
        }
        Update: {
          profile_id?: string
          request_id?: string
        }
        Relationships: [
          {
            foreignKeyName: "running_player_sessions_request_id_fkey"
            columns: ["request_id"]
            isOneToOne: false
            referencedRelation: "running_servers"
            referencedColumns: ["request_id"]
          },
          {
            foreignKeyName: "running_player_sessions_user_profile_id_fkey"
            columns: ["profile_id"]
            isOneToOne: true
            referencedRelation: "user_profiles"
            referencedColumns: ["id"]
          },
        ]
      }
      running_servers: {
        Row: {
          external_port: number
          game_id: number
          gameworld_version_num: number
          max_players: number
          public_ip: string
          request_id: string
        }
        Insert: {
          external_port: number
          game_id: number
          gameworld_version_num: number
          max_players: number
          public_ip: string
          request_id: string
        }
        Update: {
          external_port?: number
          game_id?: number
          gameworld_version_num?: number
          max_players?: number
          public_ip?: string
          request_id?: string
        }
        Relationships: [
          {
            foreignKeyName: "running_servers_game_id_fkey"
            columns: ["game_id"]
            isOneToOne: false
            referencedRelation: "game_metadata"
            referencedColumns: ["game_id"]
          },
          {
            foreignKeyName: "running_servers_game_world_fkey"
            columns: ["game_id", "gameworld_version_num"]
            isOneToOne: false
            referencedRelation: "game_worlds"
            referencedColumns: ["game_id", "version_num"]
          },
        ]
      }
      starting_player_sessions: {
        Row: {
          profile_id: string
          request_id: string
        }
        Insert: {
          profile_id: string
          request_id: string
        }
        Update: {
          profile_id?: string
          request_id?: string
        }
        Relationships: [
          {
            foreignKeyName: "starting_player_sessions_request_id_fkey"
            columns: ["request_id"]
            isOneToOne: false
            referencedRelation: "starting_servers"
            referencedColumns: ["request_id"]
          },
          {
            foreignKeyName: "starting_player_sessions_user_profile_id_fkey"
            columns: ["profile_id"]
            isOneToOne: true
            referencedRelation: "user_profiles"
            referencedColumns: ["id"]
          },
        ]
      }
      starting_servers: {
        Row: {
          game_id: number
          gameworld_version_num: number
          max_players: number
          request_id: string
          starting_at: string
        }
        Insert: {
          game_id: number
          gameworld_version_num: number
          max_players: number
          request_id: string
          starting_at: string
        }
        Update: {
          game_id?: number
          gameworld_version_num?: number
          max_players?: number
          request_id?: string
          starting_at?: string
        }
        Relationships: [
          {
            foreignKeyName: "starting_servers_game_id_fkey"
            columns: ["game_id"]
            isOneToOne: false
            referencedRelation: "game_metadata"
            referencedColumns: ["game_id"]
          },
          {
            foreignKeyName: "starting_servers_game_world_fkey"
            columns: ["game_id", "gameworld_version_num"]
            isOneToOne: false
            referencedRelation: "game_worlds"
            referencedColumns: ["game_id", "version_num"]
          },
        ]
      }
      user_likes: {
        Row: {
          created_at: string
          game_id: number
          profile_id: string
        }
        Insert: {
          created_at?: string
          game_id: number
          profile_id: string
        }
        Update: {
          created_at?: string
          game_id?: number
          profile_id?: string
        }
        Relationships: [
          {
            foreignKeyName: "user_likes_game_id_fkey"
            columns: ["game_id"]
            isOneToOne: false
            referencedRelation: "game_metadata"
            referencedColumns: ["game_id"]
          },
          {
            foreignKeyName: "user_likes_profile_id_fkey"
            columns: ["profile_id"]
            isOneToOne: false
            referencedRelation: "user_profiles"
            referencedColumns: ["id"]
          },
        ]
      }
      user_profiles: {
        Row: {
          created_at: string | null
          email: string | null
          equipped_catalog_item_id: number | null
          id: string
          user_id: number
          username: string
        }
        Insert: {
          created_at?: string | null
          email?: string | null
          equipped_catalog_item_id?: number | null
          id: string
          user_id?: number
          username: string
        }
        Update: {
          created_at?: string | null
          email?: string | null
          equipped_catalog_item_id?: number | null
          id?: string
          user_id?: number
          username?: string
        }
        Relationships: []
      }
    }
    Views: {
      [_ in never]: never
    }
    Functions: {
      search_asset_metadata: {
        Args: { input_query: string, input_asset_type: Database["public"]["Enums"]["asset_type"] | null, input_offset: number, input_limit: number }
        Returns: { asset_id: number, version_num: number, name: string, created_at: string, asset_type: Database["public"]["Enums"]["asset_type"] }[]
      }

      buy_catalog_item: {
        Args: {
          input_buyer_id: string
          input_catalog_item_id: number
        }
        Returns: undefined
      }

      delete_running_player_session_unless_play_auth_changed: {
        Args: {
          input_play_auth_hash: string
          input_profile_id: string
          input_request_id: string
        }
        Returns: undefined
      }
    }
    Enums: {
      asset_type: "Mesh" | "Texture" | "Sound" | "Object" | "Material" | "Skybox" | "Sfx"
      data_type: "mesh_data" | "texture_data" | "sound_data"
    }
    CompositeTypes: {
      [_ in never]: never
    }
  }
}

type DatabaseWithoutInternals = Omit<Database, "__InternalSupabase">

type DefaultSchema = DatabaseWithoutInternals[Extract<keyof Database, "public">]

export type Tables<
  DefaultSchemaTableNameOrOptions extends
    | keyof (DefaultSchema["Tables"] & DefaultSchema["Views"])
    | { schema: keyof DatabaseWithoutInternals },
  TableName extends DefaultSchemaTableNameOrOptions extends {
    schema: keyof DatabaseWithoutInternals
  }
    ? keyof (DatabaseWithoutInternals[DefaultSchemaTableNameOrOptions["schema"]]["Tables"] &
        DatabaseWithoutInternals[DefaultSchemaTableNameOrOptions["schema"]]["Views"])
    : never = never,
> = DefaultSchemaTableNameOrOptions extends {
  schema: keyof DatabaseWithoutInternals
}
  ? (DatabaseWithoutInternals[DefaultSchemaTableNameOrOptions["schema"]]["Tables"] &
      DatabaseWithoutInternals[DefaultSchemaTableNameOrOptions["schema"]]["Views"])[TableName] extends {
      Row: infer R
    }
    ? R
    : never
  : DefaultSchemaTableNameOrOptions extends keyof (DefaultSchema["Tables"] &
        DefaultSchema["Views"])
    ? (DefaultSchema["Tables"] &
        DefaultSchema["Views"])[DefaultSchemaTableNameOrOptions] extends {
        Row: infer R
      }
      ? R
      : never
    : never

export type TablesInsert<
  DefaultSchemaTableNameOrOptions extends
    | keyof DefaultSchema["Tables"]
    | { schema: keyof DatabaseWithoutInternals },
  TableName extends DefaultSchemaTableNameOrOptions extends {
    schema: keyof DatabaseWithoutInternals
  }
    ? keyof DatabaseWithoutInternals[DefaultSchemaTableNameOrOptions["schema"]]["Tables"]
    : never = never,
> = DefaultSchemaTableNameOrOptions extends {
  schema: keyof DatabaseWithoutInternals
}
  ? DatabaseWithoutInternals[DefaultSchemaTableNameOrOptions["schema"]]["Tables"][TableName] extends {
      Insert: infer I
    }
    ? I
    : never
  : DefaultSchemaTableNameOrOptions extends keyof DefaultSchema["Tables"]
    ? DefaultSchema["Tables"][DefaultSchemaTableNameOrOptions] extends {
        Insert: infer I
      }
      ? I
      : never
    : never

export type TablesUpdate<
  DefaultSchemaTableNameOrOptions extends
    | keyof DefaultSchema["Tables"]
    | { schema: keyof DatabaseWithoutInternals },
  TableName extends DefaultSchemaTableNameOrOptions extends {
    schema: keyof DatabaseWithoutInternals
  }
    ? keyof DatabaseWithoutInternals[DefaultSchemaTableNameOrOptions["schema"]]["Tables"]
    : never = never,
> = DefaultSchemaTableNameOrOptions extends {
  schema: keyof DatabaseWithoutInternals
}
  ? DatabaseWithoutInternals[DefaultSchemaTableNameOrOptions["schema"]]["Tables"][TableName] extends {
      Update: infer U
    }
    ? U
    : never
  : DefaultSchemaTableNameOrOptions extends keyof DefaultSchema["Tables"]
    ? DefaultSchema["Tables"][DefaultSchemaTableNameOrOptions] extends {
        Update: infer U
      }
      ? U
      : never
    : never

export type Enums<
  DefaultSchemaEnumNameOrOptions extends
    | keyof DefaultSchema["Enums"]
    | { schema: keyof DatabaseWithoutInternals },
  EnumName extends DefaultSchemaEnumNameOrOptions extends {
    schema: keyof DatabaseWithoutInternals
  }
    ? keyof DatabaseWithoutInternals[DefaultSchemaEnumNameOrOptions["schema"]]["Enums"]
    : never = never,
> = DefaultSchemaEnumNameOrOptions extends {
  schema: keyof DatabaseWithoutInternals
}
  ? DatabaseWithoutInternals[DefaultSchemaEnumNameOrOptions["schema"]]["Enums"][EnumName]
  : DefaultSchemaEnumNameOrOptions extends keyof DefaultSchema["Enums"]
    ? DefaultSchema["Enums"][DefaultSchemaEnumNameOrOptions]
    : never

export type CompositeTypes<
  PublicCompositeTypeNameOrOptions extends
    | keyof DefaultSchema["CompositeTypes"]
    | { schema: keyof DatabaseWithoutInternals },
  CompositeTypeName extends PublicCompositeTypeNameOrOptions extends {
    schema: keyof DatabaseWithoutInternals
  }
    ? keyof DatabaseWithoutInternals[PublicCompositeTypeNameOrOptions["schema"]]["CompositeTypes"]
    : never = never,
> = PublicCompositeTypeNameOrOptions extends {
  schema: keyof DatabaseWithoutInternals
}
  ? DatabaseWithoutInternals[PublicCompositeTypeNameOrOptions["schema"]]["CompositeTypes"][CompositeTypeName]
  : PublicCompositeTypeNameOrOptions extends keyof DefaultSchema["CompositeTypes"]
    ? DefaultSchema["CompositeTypes"][PublicCompositeTypeNameOrOptions]
    : never

export const Constants = {
  public: {
    Enums: {
      asset_type: ["Mesh", "Texture", "Sound", "Object", "Material", "Skybox", "Sfx"],
      data_type: ["mesh_data", "texture_data", "sound_data"],
    },
  },
} as const
