// mortiflix-engine (doc 16): the mini DAW's sound engine. JUCE + Tracktion Engine. The studio starts it and talks to it
// over stdin/stdout (JSON lines, step E2); for now it answers --version and --selftest.
//   --version    one JSON line: this engine's version and the pinned Tracktion Engine and JUCE commits
//   --selftest   renders 1 s of a 1 kHz test tone through a Tracktion Edit into memory (no sound device) and checks it
//   --serve      the studio protocol on stdin / stdout (doc 16 §4)
// Whatever JUCE, Tracktion or a plugin prints goes to stderr: stdout carries only our JSON.
#include <JuceHeader.h>
#include <iostream>
#include "Protocol.h"

namespace te = tracktion;

namespace
{
    // No sound device for the self-test: the engine must work with nothing plugged in.
    struct NoDeviceBehaviour : te::EngineBehaviour
    {
        bool autoInitialiseDeviceManager() override { return false; }
    };

    // Jobs that show a progress bar in a GUI (renders) run here on their own thread while the message loop turns.
    struct HeadlessUI : te::UIBehaviour
    {
        void runTaskWithProgressBar (te::ThreadPoolJobWithProgress& job) override
        {
            struct Runner : juce::Thread
            {
                explicit Runner (te::ThreadPoolJobWithProgress& j) : juce::Thread (j.getJobName()), job (j) { startThread(); }
                ~Runner() override { job.signalJobShouldExit(); waitForThreadToExit (10000); }
                void run() override { while (! threadShouldExit()) if (job.runJob() == juce::ThreadPoolJob::jobHasFinished) break; }
                te::ThreadPoolJobWithProgress& job;
            } runner (job);
            while (runner.isThreadRunning())
                if (! juce::MessageManager::getInstance()->runDispatchLoopUntil (10))
                    break;
        }
    };

    int protoFd = 1;
    juce::String json (const juce::var& v) { return juce::JSON::toString (v, true); }
    void emit (const juce::var& v) { auto line = json (v) + "\n"; auto s = line.toRawUTF8(); [[maybe_unused]] auto n = ::write (protoFd, s, std::strlen (s)); }

    juce::DynamicObject::Ptr versions()
    {
        juce::DynamicObject::Ptr o = new juce::DynamicObject();
        o->setProperty ("name", "mortiflix-engine");
        o->setProperty ("version", MFX_ENGINE_VERSION);
        o->setProperty ("tracktion_engine", MFX_TE_COMMIT);
        o->setProperty ("juce", MFX_JUCE_COMMIT);
        o->setProperty ("juce_version", juce::SystemStats::getJUCEVersion());
        return o;
    }

    int version() { emit (juce::var (versions().get())); return 0; }

    // One track with Tracktion's tone generator at 1 kHz, level 0.5 (−6.02 dBFS), rendered offline at 48 kHz.
    int selftest()
    {
        te::Engine engine { "mortiflix-engine", std::make_unique<HeadlessUI>(), std::make_unique<NoDeviceBehaviour>() };
        engine.getPluginManager().createBuiltInType<te::ToneGeneratorPlugin>();

        auto edit = te::Edit::createSingleTrackEdit (engine, te::Edit::EditRole::forRendering);
        edit->ensureNumberOfAudioTracks (1);
        edit->getMasterVolumePlugin()->setVolumeDb (0.0f);
        auto track = te::getAudioTracks (*edit)[0];
        track->getVolumePlugin()->setVolumeDb (0.0f);

        auto plugin = edit->getPluginCache().createNewPlugin (te::ToneGeneratorPlugin::xmlTypeName, {});
        track->pluginList.insertPlugin (plugin, 0, nullptr);
        auto tone = dynamic_cast<te::ToneGeneratorPlugin*> (plugin.get());
        tone->frequencyParam->setParameter (1000.0f, juce::dontSendNotification);
        tone->levelParam->setParameter (0.5f, juce::dontSendNotification);

        juce::BigInteger tracks;
        tracks.setBit (te::getAllTracks (*edit).indexOf (track));
        const double rate = 48000.0;
        auto stats = te::Renderer::measureStatistics ("selftest", *edit, te::TimeRange (te::TimePosition(), te::TimePosition::fromSeconds (1.0)), tracks, 512, rate);

        // a 0.5 sine at 0 dB through the track and master: peak 0.5, RMS 0.5/√2, one whole second
        const bool rendered = std::abs (stats.audioDuration - 1.0f) < 0.001f && std::abs (stats.peak - 0.5f) < 0.002f
                              && std::abs (stats.average - 0.5f / std::sqrt (2.0f)) < 0.002f;
        auto o = new juce::DynamicObject();
        o->setProperty ("ok", rendered);
        o->setProperty ("peak", stats.peak);
        o->setProperty ("peak_db", juce::Decibels::gainToDecibels (stats.peak, -150.0f));
        o->setProperty ("average", stats.average);
        o->setProperty ("seconds", stats.audioDuration);
        o->setProperty ("sample_rate", rate);
        emit (juce::var (o));
        return rendered ? 0 : 1;
    }

    // The studio protocol: commands on the message thread, until engine.quit or the studio closes stdin.
    int serve()
    {
        te::Engine engine { "mortiflix-engine", std::make_unique<HeadlessUI>(), std::make_unique<NoDeviceBehaviour>() };
        std::unique_ptr<mfx::Protocol> proto;
        auto quit = [] { juce::MessageManager::getInstance()->stopDispatchLoop(); };
        proto = std::make_unique<mfx::Protocol> (protoFd, [&] (const juce::String& cmd, const juce::var&) -> juce::var
        {
            if (cmd == "engine.hello")
            {
                auto o = versions();
                o->setProperty ("rate", 48000);
                o->setProperty ("device", juce::var());   // no device opened yet (step E3)
                return juce::var (o.get());
            }
            if (cmd == "engine.quit") { juce::MessageManager::callAsync (quit); return juce::var (new juce::DynamicObject()); }
            throw std::runtime_error (("unknown command " + cmd).toStdString());
        });
        proto->onEnd = quit;
        proto->start();
        juce::MessageManager::getInstance()->runDispatchLoop();
        proto->stop();
        return 0;
    }
}

int main (int argc, char* argv[])
{
    protoFd = mfx::Protocol::takeStdout();   // first: before JUCE, Tracktion or any plugin can print
    juce::ScopedJuceInitialiser_GUI init;
    juce::StringArray args;
    for (int i = 1; i < argc; ++i)
        args.add (argv[i]);
    if (args.contains ("--version")) return version();
    if (args.contains ("--selftest")) return selftest();
    if (args.contains ("--serve")) return serve();
    std::cerr << "mortiflix-engine: --version | --selftest | --serve" << std::endl;
    return 2;
}
