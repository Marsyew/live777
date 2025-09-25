import { useState, useRef, useEffect } from 'preact/hooks';
import { useAuth } from './AuthContext';
import { WHEPClient } from '@binbat/whip-whep/whep.js';
import { PlayIcon, StopIcon, ExclamationTriangleIcon, CameraIcon } from '@heroicons/react/24/solid';

interface LiveCamViewerProps {
  streamId: string;
  autoPlay?: boolean;
  muted?: boolean;
  reconnectDelay?: number;
  getWhepUrl?: (streamId: string) => string;
}

export function LiveCamViewer({
  streamId,
  autoPlay = true,
  muted = true,
  reconnectDelay = 5000, // Default reconnect delay in ms
  getWhepUrl,
}: LiveCamViewerProps) {
  const { token } = useAuth();
  const videoRef = useRef<HTMLVideoElement>(null);

  const whepClientRef = useRef<WHEPClient | null>(null);
  const reconnectTimerRef = useRef<number | null>(null);

  const [isConnected, setIsConnected] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [isConnecting, setIsConnecting] = useState(false);

  const handleStop = async (isGraceful = true) => {
    if (reconnectTimerRef.current) {
      clearTimeout(reconnectTimerRef.current);
      reconnectTimerRef.current = null;
    }
    if (whepClientRef.current) {
      await whepClientRef.current.stop();
      whepClientRef.current = null;
    }
    if (videoRef.current) {
      videoRef.current.srcObject = null;
    }
    setIsConnected(false);
    setIsConnecting(false);
    if (!isGraceful && reconnectDelay > 0) {
      console.log(`Connection lost. Reconnecting in ${reconnectDelay}ms...`);
      reconnectTimerRef.current = window.setTimeout(handlePlay, reconnectDelay);
    }
  };

  const handlePlay = async () => {
    if (!streamId || !token) {
        setError("Missing Stream ID or authentication Token");
        return;
    };
    if (isConnecting || isConnected) return;

    console.log(`Attempting to play stream: ${streamId}`);
    setIsConnecting(true);
    setError(null);

    try {
      const pc = new RTCPeerConnection();
      pc.addTransceiver('video', { direction: 'recvonly' });
      // pc.addTransceiver('audio', { direction: 'recvonly' });

      pc.ontrack = (event) => {
        console.log(`Received track: ${event.track.kind}`);
        if (videoRef.current) {
          if (!videoRef.current.srcObject) {
            videoRef.current.srcObject = new MediaStream();
          }
          (videoRef.current.srcObject as MediaStream).addTrack(event.track);
        }
      };

      pc.oniceconnectionstatechange = () => {
        console.log(`ICE connection state: ${pc.iceConnectionState}`);
        switch (pc.iceConnectionState) {
            case 'connected':
            case 'completed':
                setIsConnected(true);
                setIsConnecting(false);
                setError(null);
                if (reconnectTimerRef.current) {
                    clearTimeout(reconnectTimerRef.current);
                    reconnectTimerRef.current = null;
                }
                break;
            case 'failed':
            case 'disconnected':
            case 'closed':
                if (isConnected) { 
                    setError('Connection lost');
                    handleStop(false);
                }
                break;
        }
      };
      
      const whepClient = new WHEPClient();
      whepClientRef.current = whepClient;

      const url = getWhepUrl ? getWhepUrl(streamId) : `/api/whep/${streamId}`;
      console.log(`Connecting to WHEP URL: ${url}`);
      await whepClient.view(pc, url, token);
    } catch (e: any) {
      console.error('WHEP connection failed:', e);
      setError(e.message || 'Failed to start playback');
      await handleStop(true);
    }
  };

  const handleScreenshot = () => {
    if (!videoRef.current || !isConnected || videoRef.current.videoWidth === 0) {
      console.warn('Screenshot failed: Video not ready or not connected.');
      return;
    }
    const video = videoRef.current;
    const canvas = document.createElement('canvas');
    canvas.width = video.videoWidth;
    canvas.height = video.videoHeight;
    const ctx = canvas.getContext('2d');
    if (!ctx) {
        console.error('Failed to get canvas context.');
        return;
    }
    ctx.drawImage(video, 0, 0, canvas.width, canvas.height);
    const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
    const filename = `screenshot-${streamId}-${timestamp}.jpg`;
    const link = document.createElement('a');
    link.href = canvas.toDataURL('image/jpeg', 0.9);
    link.download = filename;
    document.body.appendChild(link);
    link.click();
    document.body.removeChild(link);
  };

  useEffect(() => {
    if (autoPlay) {
      handlePlay();
    }
    return () => {
      console.log('LiveCamViewer unmounting, cleaning up...');
      handleStop(true);
    };
  }, [streamId, token]); 

  return (
    <div className="bg-base-300 rounded-lg overflow-hidden shadow-lg">
      <div className="relative aspect-video">
        <video ref={videoRef} className="w-full h-full bg-black" autoPlay muted={muted} playsInline controls={false} />
        <div className={`absolute inset-0 flex items-center justify-center bg-black bg-opacity-60 transition-opacity duration-300 ${isConnected ? 'opacity-0 pointer-events-none' : 'opacity-100'}`}>
          {!isConnected && !isConnecting && error && (
            <div className="text-center text-white p-4">
              <ExclamationTriangleIcon className="h-12 w-12 text-error mx-auto mb-2" />
              <p className="font-bold">Connection Error</p>
              <p className="text-sm">{error}</p>
            </div>
          )}
          {isConnecting && <span className="loading loading-lg loading-spinner text-primary"></span>}
        </div>
      </div>
      <div className="p-4 bg-base-200 flex items-center justify-between">
        <div>
          <p className="font-bold truncate" title={streamId}>Stream ID: {streamId || 'N/A'}</p>
          <p className={`text-sm font-semibold ${isConnected ? 'text-success' : (error ? 'text-error' : 'text-warning')}`}>
            Status: {isConnected ? 'Connected' : (error ? 'Error' : (isConnecting ? 'Connecting...' : 'Disconnected'))}
          </p>
        </div>
        <div className="flex items-center gap-2">
            <button className="btn btn-ghost btn-sm btn-circle" onClick={handleScreenshot} disabled={!isConnected} title="Screenshot">
              <CameraIcon className={`h-5 w-5 ${!isConnected ? 'text-gray-600' : ''}`} />
            </button>
            {isConnected ? (
              <button className="btn btn-ghost btn-sm btn-circle" onClick={() => handleStop(true)} title="Stop">
                <StopIcon className="h-6 w-6 text-error" />
              </button>
            ) : (
              <button className="btn btn-ghost btn-sm btn-circle" onClick={handlePlay} disabled={isConnecting} title="Play">
                <PlayIcon className="h-6 w-6 text-success" />
              </button>
            )}
        </div>
      </div>
    </div>
  );
}
