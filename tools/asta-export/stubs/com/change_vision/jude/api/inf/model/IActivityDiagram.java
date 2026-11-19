// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.model;

public interface IActivityDiagram extends com.change_vision.jude.api.inf.model.IDiagram {
    public abstract com.change_vision.jude.api.inf.model.IActivity getActivity();
}
