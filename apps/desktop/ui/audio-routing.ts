export interface AudioPreferences {
  output_id: string;
  microphone_id: string;
  music_gain: number;
  microphone_gain: number;
}
export interface AudioRoutingStatus {
  supported: boolean;
  active: boolean;
  microphone_enabled: boolean;
  microphone_muted: boolean;
  message: string;
  music_peak: number;
  microphone_peak: number;
  output_peak: number;
  limited: boolean;
  underruns: number;
  dropped_frames: number;
  buffered_ms: number;
  preferences: AudioPreferences;
}
export interface AudioDevices {
  outputs: { id: string; name: string; virtual_cable: boolean }[];
  microphones: { id: string; name: string; virtual_cable: boolean }[];
}
export const initialAudio: AudioRoutingStatus = {
  supported: false,
  active: false,
  microphone_enabled: false,
  microphone_muted: false,
  message: 'Checking audio support…',
  music_peak: 0,
  microphone_peak: 0,
  output_peak: 0,
  limited: false,
  underruns: 0,
  dropped_frames: 0,
  buffered_ms: 0,
  preferences: {
    output_id: '',
    microphone_id: '',
    music_gain: 0.8,
    microphone_gain: 1,
  },
};
