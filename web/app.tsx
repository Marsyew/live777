import { useState, useRef, useEffect } from 'preact/hooks'
import Logo from '/logo.svg'
import './app.css'
import { allStream, delStream } from './api'
import { formatTime } from './utils'
import { IClientsDialog, ClientsDialog } from './dialog-clients'
import { IReforwardDialog, ReforwardDialog } from './dialog-reforward'

export function App() {
    const [streams, setStreams] = useState<any[]>([])
    const [selectedStreamId, setSelectedStreamId] = useState('')
    const refTimer = useRef<null | ReturnType<typeof setInterval>>(null)
    const refReforward = useRef<IReforwardDialog>(null)
    const refViewClients = useRef<IClientsDialog>(null)

    const updateAllStreams = async () => {
        setStreams(await allStream())
    }

    // fetch all streams on component mount
    useEffect(() => { updateAllStreams() }, [])

    const toggleTimer = () => {
        if (refTimer.current) {
            clearInterval(refTimer.current)
            refTimer.current = null
        } else {
            updateAllStreams()
            refTimer.current = setInterval(updateAllStreams, 3000)
        }
    }

    const handleViewClients = (id: string) => {
        setSelectedStreamId(id)
        refViewClients.current?.show()
    }

    const handleReforwardStream = (id: string) => {
        refReforward.current?.show(id)
    }

    return (
        <>
            <div class="flex flex-justify-center">
                <a href="https://live777.binbat.com" target="_blank">
                    <img src={Logo} class="logo" alt="Live777 logo" />
                </a>
            </div>

            <ClientsDialog ref={refViewClients} id={selectedStreamId} clients={streams.find(s => s.id == selectedStreamId)?.subscribeSessionInfos ?? []} />

            <ReforwardDialog ref={refReforward} />

            <fieldset>
                <legend class="inline-flex items-center">
                    <span>Streams (total: {streams.length})</span>
                    <label class="ml-10 inline-flex items-center cursor-pointer">
                        <input type="checkbox" value="" class="sr-only peer" checked={!!refTimer.current} onClick={toggleTimer} />
                        <div class="relative w-11 h-6 bg-gray-200 peer-focus:outline-none peer-focus:ring-4 peer-focus:ring-blue-300 dark:peer-focus:ring-blue-800 rounded-full peer dark:bg-gray-700 peer-checked:after:translate-x-full rtl:peer-checked:after:-translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:start-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all dark:border-gray-600 peer-checked:bg-blue-600"></div>
                        <span class="ml-2">Auto Refresh</span>
                    </label>
                </legend>
                <legend>
                </legend>
                <table>
                    <thead>
                        <tr>
                            <th class="mw-50">ID</th>
                            <th>Publisher</th>
                            <th>Subscriber</th>
                            <th>Reforward</th>
                            <th class="mw-300">Creation Time</th>
                            <th class="mw-300">Operation</th>
                        </tr>
                    </thead>
                    <tbody>
                        {streams.map(i =>
                            <tr>
                                <td class="text-center">{i.id}</td>
                                <td class="text-center">{i.publishLeaveTime === 0 ? "Ok" : "No"}</td>
                                <td class="text-center">{i.subscribeSessionInfos.length}</td>
                                <td class="text-center">{i.subscribeSessionInfos.filter((t: any) => t.reforward).length}</td>
                                <td class="text-center">{formatTime(i.createTime)}</td>
                                <td>
                                    <button onClick={() => delStream(i.id, i.publishSessionInfo.id)}>Destroy</button>
                                    <button onClick={() => handleViewClients(i.id)}>Clients</button>
                                    <button onClick={() => handleReforwardStream(i.id)}>Reforward</button>
                                </td>
                            </tr>
                        )}
                    </tbody>
                </table>
            </fieldset>
        </>
    )
}
