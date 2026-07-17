import React, { useState, useEffect } from 'react';
import { Switch } from '@/components/ui/switch';
import { FolderOpen, FolderCog } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';

export interface WikiPreferences {
  enabled: boolean;
  wiki_folder: string;
}

export function WikiSettings() {
  const [preferences, setPreferences] = useState<WikiPreferences>({
    enabled: false,
    wiki_folder: '',
  });
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    const loadPreferences = async () => {
      try {
        const prefs = await invoke<WikiPreferences>('get_wiki_preferences');
        setPreferences(prefs);
      } catch (error) {
        console.error('Failed to load wiki preferences:', error);
        try {
          const defaultPath = await invoke<string>('get_default_wiki_folder_path');
          setPreferences(prev => ({ ...prev, wiki_folder: defaultPath }));
        } catch (defaultError) {
          console.error('Failed to get default wiki folder path:', defaultError);
        }
      } finally {
        setLoading(false);
      }
    };
    loadPreferences();
  }, []);

  const savePreferences = async (prefs: WikiPreferences) => {
    setSaving(true);
    try {
      await invoke('set_wiki_preferences', { preferences: prefs });
      toast.success('Wiki settings saved');
    } catch (error) {
      console.error('Failed to save wiki preferences:', error);
      toast.error('Failed to save wiki settings', {
        description: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setSaving(false);
    }
  };

  const handleEnabledToggle = async (enabled: boolean) => {
    const newPreferences = { ...preferences, enabled };
    setPreferences(newPreferences);
    await savePreferences(newPreferences);
  };

  const handleChooseFolder = async () => {
    try {
      const selected = await invoke<string | null>('select_wiki_folder');
      if (selected) {
        const newPreferences = { ...preferences, wiki_folder: selected };
        setPreferences(newPreferences);
        await savePreferences(newPreferences);
      }
    } catch (error) {
      console.error('Failed to select wiki folder:', error);
      toast.error('Failed to select folder');
    }
  };

  const handleOpenFolder = async () => {
    try {
      await invoke('open_wiki_folder');
    } catch (error) {
      console.error('Failed to open wiki folder:', error);
      toast.error('Failed to open folder');
    }
  };

  if (loading) {
    return (
      <div className="animate-pulse">
        <div className="h-4 bg-gray-200 rounded w-1/4 mb-4"></div>
        <div className="h-8 bg-gray-200 rounded mb-4"></div>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <div>
        <h3 className="text-lg font-semibold mb-4">Wiki Export</h3>
        <p className="text-sm text-gray-600 mb-6">
          Automatically write each completed meeting summary and transcript to a markdown file
          in a folder of your choice, so it can be picked up by your knowledge base or notes vault.
        </p>
      </div>

      <div className="flex items-center justify-between p-4 border rounded-lg">
        <div className="flex-1">
          <div className="font-medium">Send summaries to Wiki</div>
          <div className="text-sm text-gray-600">
            Write a markdown file automatically whenever a meeting summary finishes generating
          </div>
        </div>
        <Switch
          checked={preferences.enabled}
          onCheckedChange={handleEnabledToggle}
          disabled={saving}
        />
      </div>

      {preferences.enabled && (
        <div className="p-4 border rounded-lg bg-gray-50">
          <div className="font-medium mb-2">Wiki Folder</div>
          <div className="text-sm text-gray-600 mb-3 break-all">
            {preferences.wiki_folder || 'Default folder'}
          </div>
          <div className="flex gap-2">
            <button
              onClick={handleChooseFolder}
              className="flex items-center gap-2 px-3 py-2 text-sm border border-gray-300 rounded-md hover:bg-gray-50 transition-colors"
              disabled={saving}
            >
              <FolderCog className="w-4 h-4" />
              Choose Folder
            </button>
            <button
              onClick={handleOpenFolder}
              className="flex items-center gap-2 px-3 py-2 text-sm border border-gray-300 rounded-md hover:bg-gray-50 transition-colors"
            >
              <FolderOpen className="w-4 h-4" />
              Open Folder
            </button>
          </div>
        </div>
      )}

      {!preferences.enabled && (
        <div className="p-4 border rounded-lg bg-yellow-50">
          <div className="text-sm text-yellow-800">
            Wiki export is off. Turn on "Send summaries to Wiki" to start writing meeting notes to your wiki folder.
          </div>
        </div>
      )}
    </div>
  );
}
