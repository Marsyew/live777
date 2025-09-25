import { useState, useRef} from 'preact/hooks';
import { Input } from 'react-daisyui';
import { LiveCamViewer } from './components/LiveViewer';
import { PlaybackViewer } from './components/Playback'; 
import { useAuth } from './components/AuthContext';
import { ChangePassword, type ChangePasswordRef } from './components/ChangePassword';

export function LiveCamPage(_props: { path: string }) {
    const [streamId, setStreamId] = useState('demo');
    const [mode, setMode] = useState<'live' | 'playback'>('live'); 
    const { logout } = useAuth();

    const changePasswordModalRef = useRef<ChangePasswordRef>(null);

    return (
        <div className="min-h-screen bg-base-100 text-base-content">
            <div className="container mx-auto p-4 md:p-8">
                <div className="flex justify-between items-center mb-6">
                    <h1 className="text-3xl font-bold">LiveCam</h1>
                    <div className="flex items-center gap-2">
                        <button 
                            onClick={() => changePasswordModalRef.current?.show()} 
                            className="btn btn-outline btn-sm"
                        >
                            Change Password
                        </button>
                        <button onClick={logout} className="btn btn-outline btn-sm">Logout</button>
                    </div>
                </div>

                <div className="mb-6 flex flex-wrap gap-4 items-end">
                    <div className="form-control">
                        <label className="label">
                            <span className="label-text">Enter Stream ID</span>
                        </label>
                        <Input
                            className="w-full max-w-xs"
                            value={streamId}
                            onInput={(e) => setStreamId(e.currentTarget.value)}
                            placeholder="e.g., demo"
                        />
                    </div>
                    <div className="join">
                        <button 
                            className={`btn join-item ${mode === 'live' ? 'btn-active' : ''}`}
                            onClick={() => setMode('live')}>
                            Live
                        </button>
                        <button 
                            className={`btn join-item ${mode === 'playback' ? 'btn-active' : ''}`}
                            onClick={() => setMode('playback')}>
                            Playback
                        </button>
                    </div>
                </div>

                {streamId ? (
                    <div>
                        {mode === 'live' ? (
                            <LiveCamViewer streamId={streamId} />
                        ) : (
                            <PlaybackViewer streamId={streamId} />
                        )}
                    </div>
                ) : (
                    <div className="text-center p-10 bg-base-200 rounded-lg">
                        <p>Enter a Stream ID to start.</p>
                    </div>
                )}
            </div>
             <ChangePassword ref={changePasswordModalRef} />
        </div>
    );
}
